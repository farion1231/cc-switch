with open('.github/workflows/release.yml', 'r', encoding='utf-8') as f:
    base = f.read()
with open('.github/workflows/publish-release-job.yml', 'r', encoding='utf-8') as f:
    publish = f.read()
with open('.github/workflows/release.yml', 'w', encoding='utf-8') as f:
    f.write(base.rstrip() + '\n\n' + publish.lstrip())
print('Done, lines:', base.count(chr(10)) + publish.count(chr(10)) + 1)
