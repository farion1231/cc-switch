export class PermanentError extends Error {
  constructor(message) {
    super(message);
    this.name = "PermanentError";
  }
}

export class RetryableError extends Error {
  constructor(message) {
    super(message);
    this.name = "RetryableError";
  }
}

function hasLoneSurrogate(value) {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return true;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      return true;
    }
  }
  return false;
}

function timestampMs(value, field) {
  if (typeof value === "number") {
    if (!Number.isFinite(value) || value < 0 || value > 1e12) {
      throw new PermanentError(
        `${field} must be Unix seconds, not milliseconds.`,
      );
    }
    return Math.round(value * 1000);
  }
  if (typeof value === "string") {
    // Keep this narrower than Date.parse: it silently rolls 2030-02-29 into March.
    const match =
      /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(Z|([+-])(\d{2}):(\d{2}))$/i.exec(
        value,
      );
    if (!match) {
      throw new PermanentError(`${field} must be ISO-8601 with a timezone.`);
    }
    const [
      ,
      yearText,
      monthText,
      dayText,
      hourText,
      minuteText,
      secondText,
      ,
      ,
      offsetHourText,
      offsetMinuteText,
    ] = match;
    const year = Number(yearText);
    const month = Number(monthText);
    const day = Number(dayText);
    const hour = Number(hourText);
    const minute = Number(minuteText);
    const second = Number(secondText);
    const offsetHour =
      offsetHourText === undefined ? 0 : Number(offsetHourText);
    const offsetMinute =
      offsetMinuteText === undefined ? 0 : Number(offsetMinuteText);
    const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
    const daysPerMonth = [
      31,
      leapYear ? 29 : 28,
      31,
      30,
      31,
      30,
      31,
      31,
      30,
      31,
      30,
      31,
    ];
    if (
      month < 1 ||
      month > 12 ||
      day < 1 ||
      day > daysPerMonth[month - 1] ||
      hour > 23 ||
      minute > 59 ||
      second > 59 ||
      offsetHour > 23 ||
      offsetMinute > 59
    ) {
      throw new PermanentError(`${field} is not a valid RFC 3339 timestamp.`);
    }
    const parsed = Date.parse(value);
    if (Number.isNaN(parsed))
      throw new PermanentError(`${field} is not a valid ISO-8601 timestamp.`);
    return parsed;
  }
  throw new PermanentError(
    `${field} must be Unix seconds or an ISO-8601 timestamp.`,
  );
}

export function validateRefreshResult(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new PermanentError("Provider response must be an object.");
  }
  const { apiKey, expiresAt, refreshAfter, message } = value;
  if (
    typeof apiKey !== "string" ||
    !apiKey ||
    /[\s\p{White_Space}]/u.test(apiKey) ||
    hasLoneSurrogate(apiKey)
  ) {
    throw new PermanentError(
      "Provider response needs a non-empty apiKey without whitespace.",
    );
  }
  const output = { apiKey };
  if (expiresAt !== undefined) {
    const expiresMs = timestampMs(expiresAt, "expiresAt");
    if (expiresMs <= Date.now())
      throw new PermanentError("Provider returned an expired API key.");
    output.expiresAt = new Date(expiresMs).toISOString();
  }
  if (refreshAfter !== undefined) {
    const refreshMs = timestampMs(refreshAfter, "refreshAfter");
    output.refreshAfter =
      typeof refreshAfter === "number"
        ? refreshAfter
        : new Date(refreshMs).toISOString();
  }
  if (message !== undefined) {
    if (typeof message !== "string" || hasLoneSurrogate(message))
      throw new PermanentError("message must be a string.");
    output.message = Array.from(message).slice(0, 500).join("");
  }
  return output;
}
