# Interoperability data

Tables an implementation needs to address keys and LEDs. Use them as-is.

| File | Device | Columns |
|---|---|---|
| `everest_max_keys.csv` | Everest Max | `DLLKeyId` firmware key code · `DLLMatrixIndex` LED index · `KeyId` Windows VK code (Base Camp's macro recorder) · `KeyName` (US layout) |
| `everest_max_layout.csv` | Everest Max | `index` layout/LED index · `hid_usage` bit in the key-state bitmap · `ui_row`, `ui_col` drawing position · `led` LED index · `fw_key` firmware key code · `label` · `sync_pos` position in the sync colour frame |
| `macropad_layout.csv` | MacroPad | same columns as `everest_max_layout.csv`; 12 keys. `index` is the per-key lighting slot (0..11), `led` the matrix number used by events and bindings |
| `displaypad_layout.csv` | DisplayPad | identical to the MacroPad table; `index` is the icon index (0..11) |
| `everest60_keys.csv` | Everest 60 | base layer: `DLLKeyId` source/target key id · `DLLMatrixIndex` event/LED id · `KeyId` Windows VK · `KeyName` · `LayerType` (empty = base) |
| `everest60_fn_keys.csv` | Everest 60 | Fn layer, same columns, `LayerType = 3` |
