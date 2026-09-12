path = r"C:\Users\admin\Desktop\cc-switch\src\components\ui\checkbox.tsx"
with open(path, "r", encoding="utf-8") as f:
    content = f.read()

old = '          className={cn(\n            "w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2",\n            className,\n          )}'
new = '          className={cn(\n            "w-4 h-4 text-blue-500 bg-white dark:bg-gray-800 border-border-default rounded focus:ring-blue-500 dark:focus:ring-blue-400 focus:ring-2",\n            props.disabled && "opacity-50 cursor-not-allowed",\n            className,\n          )}'
content = content.replace(old, new)
with open(path, "w", encoding="utf-8") as f:
    f.write(content)
print("Checkbox disabled styling added")
