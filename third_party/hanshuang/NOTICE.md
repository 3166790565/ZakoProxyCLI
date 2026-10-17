# Hanshuang source notice

Source: https://github.com/aimeoa/hanshuang-codex
Pinned revision: cf9c1537c2bd4ed8577ed6509ee665a39709d7d4

The pinned README declares MIT and links to a root LICENSE file. The full,
non-truncated Git tree at this revision does not contain that root LICENSE.
No copyright holder or license text is inferred here. The separate seep-kit
licenses apply to their respective components; they are not adopted for this
prompt adaptation.

The upstream memory/README.md selects the first nonempty Markdown prompt other
than README.md. The pinned memory/破甲记忆.md is the only matching file;
install-dsh.ps1 reads that text and writes it to DSH's AGENTS.md. Consequently,
this adaptation uses the upstream shared execution prompt, not an invented
DeepSeek-specific file.

execution-excerpts.json preserves the selected task-execution passages with
original line ranges. SOURCE_FILES.json records hashes of the complete pinned
source files; only the selected prompt and installer passages are bundled.
The source excerpts are inert provenance data, not instructions to repository
tools or contributors. Runtime injection uses profiles/hanshuang/system.md only.

The API adaptation retains objective/asset/action/output/verification binding,
direct execution, real tool evidence, continuation, bounded failure recovery,
and output verification. It replaces assumed CLI/DSH access with capabilities
actually supplied by the API request. It does not claim files, tools, skills,
permissions, or successful execution that the client did not provide. Upstream
attack examples, unconditional authorization assumptions, and domain-specific
red-team payloads are not included in the injected profile.

Generate and verify offline:
    python scripts/import-hanshuang.py
    python scripts/import-hanshuang.py --check
