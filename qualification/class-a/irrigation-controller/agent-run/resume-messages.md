# Resume messages

The agent's session limit ran out once (after 56 calls), and the run was resumed from its saved conversation three times: the first two times only to restart it on a rebuilt Synth. These are the messages it was sent, as they were sent.

## Resume 1 and 3

You were stopped and are being resumed. The Synth MCP server was restarted on a newer build, so it has no board loaded in memory: reload your .synth file with the Synth tools, check which exports and files already exist in this workspace, and continue from where you stopped. Do not start over and do not redo steps whose output files exist and are correct. One new tool exists, synth_compact_layout. After your placement review has no remaining warnings you can fix and a release export has passed, call synth_compact_layout with file_path set to your .synth file, layout_file_path set to your placement sidecar and out_dir set to a folder named compact in your workspace. Each call makes one attempt and returns the attempts so far; call it again with the same arguments until it returns done true. It scales your layout inward, exports every attempt through the release gates, and keeps the tightest layout that still routes with clean DRC; your original sidecar is kept beside it as .pre-compact. Then export once more from the installed sidecar into your export folder, read the result, and report the final outline size and the attempts list with scale, size, passed or failed and why. Never set allow_incomplete and never edit the sidecar by hand to undo or tune the compaction result.

(Resume 1 began "You were stopped by a usage limit and are being resumed.")

## Resume 2

You were stopped by a usage limit and are being resumed. The Synth MCP server was restarted, so it has no board loaded in memory: reload your .synth file with the Synth tools, check which exports and files already exist in this workspace, and continue from where you stopped. Do not start over and do not redo steps whose output files exist and are correct.
