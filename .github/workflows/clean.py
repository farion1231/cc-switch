import re

with open(".github/workflows/release-base.yml", "r", encoding="utf-8") as f:
    content = f.read()

# Remove macOS from matrix
content = content.replace("          - os: macos-14\n", "")

# Remove macOS step blocks
# Pattern: - name: <something> + if: runner.os == 'macOS' + body
lines = content.split("\n")
result = []
skip = False

for i, line in enumerate(lines):
    stripped = line.lstrip()
    
    # Skip macOS matrix entry
    if "os: macos-14" in stripped:
        continue
    
    # Skip macOS conditional steps
    if stripped.startswith("if:") and "runner.os == 'macOS'" in stripped:
        skip = True
        continue
    
    # Skip step names with macOS
    if stripped.startswith("- name:") and ("macOS" in stripped or "Apple signing" in stripped or "Notarize" in stripped or "Verify macOS" in stripped or "Clean up Apple" in stripped):
        skip = True
        continue
    
    # If skipping, skip until we hit a new step or non-indented line
    if skip:
        if stripped and not stripped.startswith(" ") and not stripped.startswith("\t"):
            # New block started
            skip = False
            result.append(line)
        elif stripped.startswith("- name:"):
            # Next step at same level
            skip = False
            result.append(line)
        else:
            continue
    else:
        result.append(line)

cleaned = "\n".join(result)

# Read publish job
with open(".github/workflows/publish-release-job.yml", "r", encoding="utf-8") as f:
    publish = f.read()

# Combine
final = cleaned.rstrip() + "\n\n" + publish.lstrip()

with open(".github/workflows/release.yml", "w", encoding="utf-8") as f:
    f.write(final)

print("Done, lines:", len(final.splitlines()))
