const { ClassicLevel } = require('classic-level');
const path = require('path');

const dbPath = process.argv[2];
const customModels = (process.argv[3] || '')
  .split(',')
  .map((s) => s.trim())
  .filter(Boolean);

function be16(str) {
  const parts = [];
  for (const ch of str) {
    parts.push(0x00, ch.charCodeAt(0));
  }
  return Buffer.from(parts);
}

function decodeMaybeUTF16(buf) {
  const options = [];
  for (const slice of [buf, buf.subarray(1)]) {
    if (slice.length % 2 !== 0) continue;
    options.push(Buffer.from(slice).swap16().toString('utf16le'));
    options.push(Buffer.from(slice).toString('utf16le'));
  }
  const prefer = options.filter((t) => t.includes('"') && t.includes('{'));
  return prefer[0] || options[0] || buf.toString('utf8');
}

function encodeUTF16BE(txt) {
  return be16(txt);
}

function verifyEvalValue(buf, keyName) {
  const candidates = [];
  for (const slice of [buf.subarray(0, buf.length - (buf.length % 2)), buf.subarray(1)]) {
    if (slice.length % 2 === 0) {
      candidates.push(Buffer.from(slice).swap16().toString('utf16le'));
      candidates.push(Buffer.from(slice).toString('utf16le'));
    }
  }
  candidates.push(buf.toString('utf8'));
  const txt = candidates.find((t) => t.includes('statsig') || t.includes('use_hidden_models')) || buf.toString('utf8');
  const hiddenOk = txt.includes('\\"use_hidden_models\\":false');
  const modelsOk = customModels.length === 0 || customModels.every((m) => txt.includes(m));
  if (!hiddenOk || !modelsOk) {
    throw new Error(`${keyName}: verification failed (hidden_false=${hiddenOk}, models=${modelsOk})`);
  }
  return { hiddenOk, modelsOk };
}

async function main() {
  const db = new ClassicLevel(dbPath, {
    createIfMissing: false,
    errorIfExists: false,
    valueEncoding: 'buffer',
    keyEncoding: 'buffer',
  });
  await db.open();

  const reports = [];
  const evalKeys = [];
  for await (const [k] of db.iterator()) {
    const ks = k.toString('latin1');
    if (ks.includes('statsig.cached.evaluations.')) {
      evalKeys.push(ks);
    }
  }

  // The Statsig localStorage value is an outer JSON object whose "data"
  // field is itself a JSON string, so quotes appear backslash-escaped.
  const truePat = be16('\\"use_hidden_models\\":true');
  const falsePat = be16('\\"use_hidden_models\\":false');
  const availStart = be16('\\"available_models\\":[');
  const insertText = customModels.map((m) => `\\"${m}\\"`).join(',') + ',';
  const insertPat = be16(insertText);

  for (const ks of evalKeys) {
    const key = Buffer.from(ks, 'latin1');
    let val = await db.get(key);
    let changed = false;
    let replaced = 0;

    let buf = val;
    while (true) {
      const i = buf.indexOf(truePat);
      if (i < 0) break;
      buf = Buffer.concat([buf.subarray(0, i), falsePat, buf.subarray(i + truePat.length)]);
      replaced += 1;
      changed = true;
    }

    if (customModels.length > 0) {
      const ai = buf.indexOf(availStart);
      if (ai >= 0) {
      const after = buf.subarray(ai + availStart.length, ai + availStart.length + 400).toString('latin1');
        const already = customModels.every((m) => after.includes(m));
        if (!already) {
          buf = Buffer.concat([
            buf.subarray(0, ai + availStart.length),
            insertPat,
            buf.subarray(ai + availStart.length),
          ]);
          changed = true;
        }
      }
    }

    if (changed) {
      await db.put(key, buf);
      const reread = await db.get(key);
      verifyEvalValue(reread, ks);
      const txt = decodeMaybeUTF16(buf);
      reports.push(`${ks}: use_hidden_models replacements=${replaced}, custom models inserted=${customModels.length > 0}, VERIFIED`);
    } else {
      const reread = await db.get(key);
      verifyEvalValue(reread, ks);
      reports.push(`${ks}: already false`);
    }
  }

  // Bump the last-modified timestamps so a remote refresh does not immediately
  // overwrite the patched cache.
  const lmKeyName = 'statsig.last_modified_time.evaluations';
  const lmKey = Buffer.from('_app://-\u0000\u0001' + lmKeyName, 'latin1');
  try {
    const lmVal = await db.get(lmKey);
    const future = Date.now() + 30 * 24 * 60 * 60 * 1000;
    const marker = lmVal[0] === 0x01 ? 1 : 0;
    const body = lmVal.subarray(marker);
    let parsed = null;
    let scheme = null;
    for (const candidate of [
      { scheme: 'utf8', text: body.toString('utf8') },
      { scheme: 'be', text: body.length % 2 === 0 ? Buffer.from(body).swap16().toString('utf16le') : null },
      { scheme: 'le', text: body.length % 2 === 0 ? Buffer.from(body).toString('utf16le') : null },
    ]) {
      if (!candidate.text) continue;
      try {
        parsed = JSON.parse(candidate.text);
        scheme = candidate.scheme;
        break;
      } catch (e) {
        // try the next byte order
      }
    }
    if (parsed && scheme) {
      for (const ks of evalKeys) {
        parsed[ks.replace(/^_app:\/\/-\u0000\u0001/, '')] = future;
      }
      const json = JSON.stringify(parsed);
      let newBody;
      if (scheme === 'utf8') newBody = Buffer.from(json, 'utf8');
      else if (scheme === 'be') newBody = be16(json);
      else newBody = Buffer.from(json, 'utf16le');
      if (marker) newBody = Buffer.concat([Buffer.from([0x01]), newBody]);
      await db.put(lmKey, newBody);
      const reread = await db.get(lmKey);
      const rereadBody = reread.subarray(reread[0] === 0x01 ? 1 : 0);
      const rereadText = rereadBody.toString('utf8').includes('statsig')
        ? rereadBody.toString('utf8')
        : (rereadBody.length % 2 === 0 ? Buffer.from(rereadBody).swap16().toString('utf16le') : '');
      const rereadObj = JSON.parse(rereadText);
      const ok = evalKeys.every((ks) => Number(rereadObj[ks.replace(/^_app:\/\/-\u0000\u0001/, '')]) > Date.now());
      if (!ok) throw new Error(`${lmKeyName}: verification failed`);
      reports.push(`${lmKeyName}: timestamps bumped to ${future} (${scheme}), VERIFIED`);
    } else {
      reports.push(`${lmKeyName}: skipped (could not parse timestamp entry)`);
    }
  } catch (e) {
    reports.push(`${lmKeyName}: skipped (${e.message})`);
  }

  await db.close();
  console.log(reports.join('\n'));
}

main().catch((e) => {
  console.error('PATCH FAILED:', e);
  process.exit(1);
});
