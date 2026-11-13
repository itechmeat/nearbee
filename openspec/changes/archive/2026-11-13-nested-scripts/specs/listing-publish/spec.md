# Spec Delta

## ADDED Requirements

### Requirement: Script collection
Publishing SHALL bundle every regular file under the source playbook's `scripts/` directory, in its subdirectories too, each named by its path relative to `scripts/` with `/` separators and sorted by that path, and SHALL refuse to publish when any entry under `scripts/` is a symbolic link, naming it.

#### Scenario: Nested files
- **WHEN** the source has `scripts/run.sh`, `scripts/lib/a.py` and `scripts/prompts/p.md`
- **THEN** the bundle holds `lib/a.py`, `prompts/p.md` and `run.sh`, in that order, with their bytes and executable flags

#### Scenario: Flat directory unchanged
- **WHEN** the source's `scripts/` holds only top-level files
- **THEN** the bundle names and the derived version are exactly those before this change

#### Scenario: Symlink refused
- **WHEN** `scripts/lib/link.py` is a symbolic link
- **THEN** publishing fails naming `lib/link.py` and nothing is installed
