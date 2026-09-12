import re

with open(r"C:\Users\admin\Desktop\cc-switch\.github\workflows\release.yml", "r", encoding="utf-8") as f:
    content = f.read()

# Remove macos-14 from matrix
content = re.sub(r"\s+- os: macos-14", "", content)

# Remove macOS steps (using non-greedy matching)
patterns = [
    # Add macOS targets
    (r"\n\s+- name: Add macOS targets\n\s+if: runner\.os == 'macOS'\n\s+run: \|\n\s+rustup target add aarch64-apple-darwin x86_64-apple-darwin\n", ""),
    # Import Apple signing certificate
    (r"\n\s+- name: Import Apple signing certificate\n\s+if: runner\.os == 'macOS'\n\s+shell: bash\n\s+run: \|\n.*?(?=\n\s+- name:)", "\n"),
    # Build Tauri App (macOS)
    (r"\n\s+- name: Build Tauri App \(macOS\)\n\s+if: runner\.os == 'macOS'\n.*?(?=\n\s+- name:)", "\n"),
    # Prepare macOS Assets
    (r"\n\s+- name: Prepare macOS Assets\n\s+if: runner\.os == 'macOS'\n.*?(?=\n\s+- name:)", "\n"),
    # Notarize macOS DMG
    (r"\n\s+- name: Notarize macOS DMG\n\s+if: runner\.os == 'macOS'\n.*?(?=\n\s+- name:)", "\n"),
    # Verify macOS code signing
    (r"\n\s+- name: Verify macOS code signing and notarization\n\s+if: runner\.os == 'macOS'\n.*?(?=\n\s+- name:)", "\n"),
    # Clean up Apple signing keychain
    (r"\n\s+- name: Clean up Apple signing keychain\n\s+if: runner\.os == 'macOS'.*?(?=\n\s+- name:|\Z)", "\n"),
]

for pattern, replacement in patterns:
    content = re.sub(pattern, replacement, content, flags=re.DOTALL)

# Remove macOS from release notes
content = re.sub(r"            - \*\*macOS\*\*: .*?\n", "", content)
content = re.sub(r"            - \*macOS 版本已通过.*?\n", "", content)

with open(r"C:\Users\admin\Desktop\cc-switch\.github\workflows\release.yml", "w", encoding="utf-8") as f:
    f.write(content)

print("Done removing macOS from workflow")
