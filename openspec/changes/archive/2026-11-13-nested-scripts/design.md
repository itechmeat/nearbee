# Design: nested scripts

1. Replace the flat `read_dir` in `publish.rs` (the source loader that
   returns `(text, scripts)`) with a recursive walk using `std::fs`
   (`symlink_metadata`, no new dependency): directories are descended,
   regular files collected, a symlink at any depth is an error
   `"scripts/<rel>: a symbolic link is not allowed"`, other file types are
   skipped. Names are relative paths joined with `/`; the list is sorted by
   name.
2. `derived_version` and `build_bundle` take the names as they are; a flat
   directory gives the same names as today, so existing versions hold.
3. Tests: the three scenarios, plus the existing publish tests unchanged.
4. Docs: `docs/apb.md`, the publish install paragraph, one sentence.
