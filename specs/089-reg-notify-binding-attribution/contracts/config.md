# Contract: configuration

One new key, settable at section level and overridable per line (FR-010).

| Key | Type | Default | Range |
|---|---|---|---|
| `[vowifi].reg_rejected_cooldown_sec` | integer | `1800` | `60..=86400` |
| `[[vowifi.line]].reg_rejected_cooldown_sec` | integer, optional | inherits `[vowifi]` | `60..=86400` |
| `[volte].reg_rejected_cooldown_sec` | integer, optional | inherits `[vowifi]` | `60..=86400` |
| `[[volte.line]].reg_rejected_cooldown_sec` | integer, optional | inherits `[volte]` | `60..=86400` |

Out of range → config error naming the key and the accepted range (strict parsing, like every other key).

Effective cooldown after a `rejected` = `max(configured, network retry-after)`.

`config.toml.example` and `docs/configuration.md` document the key under both sections, commented out at its
default.
