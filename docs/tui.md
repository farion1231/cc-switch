# CC-Switch TUI

Launch:

```bash
cc-switch cmd
```

## Keyboard shortcuts

Main view:
- `↑/↓` or `j/k`: move selection
- `Tab` / `Shift+Tab`: switch tool tab
- `Enter`: switch to selected provider
- `a`: add provider
- `e`: edit provider
- `d`: delete provider
- `/`: search/filter providers
- `?`: help
- `q`: quit

Forms:
- `Tab` / `Shift+Tab`: next/previous field
- `Esc`: cancel
- `Enter`: open/select (save on the Save/Update button)

Codex `config.toml` editor:
- `Ctrl+S`: save (validates TOML)
- `Esc`: cancel

## Screenshot (ASCII)

```text
┌──────────────────────────────────────────────────────────────────────────┐
│  CC-Switch TUI  ⠋                     [Claude] [Codex] [Gemini]           │
├───────────────────────────────┬──────────────────────────────────────────┤
│  Providers                    │  Provider Details                         │
│  ───────────────────────────  │  ───────────────────────────────────────  │
│  ▶ ● OpenRouter [Active]      │  Name:    OpenRouter                       │
│    ○ Anthropic Direct         │  ID:      openrouter-...                   │
│    ○ Custom API               │  Base:    https://openrouter.ai/api        │
│                               │  Model:   claude-sonnet-4.5                │
│                               │  Key:     sk-****...****                   │
├───────────────────────────────┴──────────────────────────────────────────┤
│  ↑/↓ Move  Tab Tool  Enter Switch  a Add  e Edit  d Delete  / Search  q    │
└──────────────────────────────────────────────────────────────────────────┘
```

