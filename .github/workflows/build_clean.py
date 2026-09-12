import re

# Read the base workflow
with open('.github/workflows/release-base.yml', 'r', encoding='utf-8') as f:
    lines = f.readlines()

result = []
skip_block = False
block_indent = 0

for line in lines:
    stripped = line.lstrip()
    indent = len(line) - len(stripped)
    
    # Detect macOS conditional steps to skip
    if stripped.startswith('if:') and "runner.os == 'macOS'" in stripped:
        skip_block = True
        block_indent = indent
        continue
    
    # If skipping, check if we've hit a new step at same or lower indent
    if skip_block:
        # Check if this is a new step at the same indentation level
        if stripped.startswith('- name:') and indent <= block_indent:
            skip_block = False
            result.append(line)
        elif stripped and indent < block_indent:
            # We've exited the current block
            skip_block = False
            result.append(line)
        else:
            # Skip this line
            continue
    
    result.append(line)

# Write cleaned content
cleaned = ''.join(result)

# Read the publish job
with open('.github/workflows/publish-release-job.yml', 'r', encoding='utf-8') as f:
    publish_job = f.read()

# Combine
final = cleaned.rstrip() + '\n\n' + publish_job.lstrip()

with open('.github/workflows/release.yml', 'w', encoding='utf-8') as f:
    f.write(final)

print(f'Done! Total lines: {len(final.splitlines())}')
