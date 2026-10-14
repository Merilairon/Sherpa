# Interoperability data

Tables an implementation needs to address keys and LEDs. Use them as-is.

| File | Device | Columns |
|---|---|---|
| `everest_max_keys.csv` | Everest Max | `DLLKeyId` firmware key code · `DLLMatrixIndex` LED index · `KeyId` Windows VK code (Base Camp's macro recorder) · `KeyName` (US layout) |
| `everest_max_layout.csv` | Everest Max | `index` layout/LED index · `hid_usage` bit in the key-state bitmap · `ui_row`, `ui_col` drawing position · `led` LED index · `fw_key` firmware key code · `label` · `sync_pos` position in the sync colour frame |
