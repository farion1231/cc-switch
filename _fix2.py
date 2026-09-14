path = r"C:\Users\admin\Desktop\cc-switch\src\components\providers\forms\CodexFormFields.tsx"
with open(path, "r", encoding="utf-8") as f:
    content = f.read()

# Fix text checkbox: explicit checked={true}, aria-disabled, better disabled styling
old = '      <label className="inline-flex items-center gap-1.5 cursor-not-allowed text-xs text-foreground">\n        <Checkbox\n          checked\n          disabled'
new = '      <label className="inline-flex items-center gap-1.5 text-xs text-foreground opacity-70">\n        <Checkbox\n          checked={true}\n          disabled\n          aria-disabled'
content = content.replace(old, new)
with open(path, "w", encoding="utf-8") as f:
    f.write(content)
print("Text checkbox fixed")
