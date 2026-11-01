# Backup: to be deleted later

Old material kept until we are sure it is no longer needed. Nothing here is used by Hive-Rail at run time.

| Folder | What it was | Before deleting it |
| --- | --- | --- |
| `examples-layout-v2/` | The example meeting and modules in module schema version 2 (track pieces with lengths, hand-written geometry), as they were before the modules were drawn from SpDrS60 tiles. `examples/layout/` now holds the same modules as tile modules (schema version 3). | Most of the test suite still runs on these files (`src/layout.rs`, `layout_sim.rs`, `layout_view.rs`, `layout_share.rs`, `topology.rs`: their tests read `backup/examples-layout-v2`). Move those tests to the tile modules in `examples/layout/` first; `bench-double-track.json` has no tile version yet. |
| `tools/convert_modules_v3.py` | The one-off conversion of the version 2 example modules into tile modules: track plans, DB/FREMO names, signal placement, occupancy sections. The modules are edited in the module editor (`/panel`) from now on. | Nothing: it only reads `examples-layout-v2/`. |

Schema version 2 itself is still read by the loader (`src/layout.rs`), so older module files keep working.
