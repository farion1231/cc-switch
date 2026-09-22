import re

with open('.github/workflows/release-base.yml', 'r', encoding='utf-8') as f:
    content = f.read()

lines = content.split('\n')
result = []
skip_until_next_step = False
step_indent = 0

for i, line in enumerate(lines):
    stripped = line.lstrip()
    indent = len(line) - len(stripped)
    
    # Check if this is a macOS conditional
    if stripped.startswith('if:') and "runner.os == 'macOS'" in stripped:
        skip_until_next_step = True
        step_indent = indent
        continue
    
    # If we're skipping, check if we've hit the next step
    if skip_until_next_step:
        if stripped.startswith('- name:') and indent <= step_indent:
            skip_until_next_step = False
            result.append(line)
        elif stripped and indent < step_indent:
            skip_until_next_step = False
            result.append(line)
        else:
            continue
    
    result.append(line)

# Write to release.yml
with open('.github/workflows/release.yml', 'w', encoding='utf-8') as f:
    f.write('\n'.join(result))

# Append publish job
with open('.github/workflows/publish-release-job.yml', 'r', encoding='utf-8') as f:
    publish = f.read()
with open('.github/workflows/release.yml', 'a', encoding='utf-8') as f:
    f.write('\n' + publish)

print('Done, lines:', len(result))
