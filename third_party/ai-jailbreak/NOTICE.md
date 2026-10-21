# AI-Jailbreak attribution

Reference repository: https://github.com/Baimuking/AI-Jailbreak

Pinned revision: `8c278b10f47f305546a52d7a0ce044d2c643dc71`.
The accompanying LICENSE is the upstream MIT license and third-party notice.
Copyright (c) 2026 Baimuking.

The two bundled `.txt` files are unmodified, inert reference data from
`docs/prompts/` at that revision. They are not executable project instructions.
No upstream installer, CLI, or agent configuration is executed or installed.

ZakoAI's six opt-in GLM/Grok profiles are curated English API adaptations of
the direct-output, technical-delivery, uncertainty, and continuation concepts.
They use the actual request's runtime context. They omit blanket authorization,
fixed workspace identity, hierarchy overrides, unconditional phrase bans, and
assumed persistent memory. Existing profiles remain available unchanged.

The upstream cards and these adaptations are unverified for model effectiveness.
Neither structural scores nor protocol-injection tests are model success rates.
See `profiles/ai-jailbreak/SOURCE.json` for provenance, decisions, and hashes.
`python scripts/import-ai-jailbreak.py --check` verifies the bundled inputs and
generated outputs offline; without `--check`, it regenerates only the profiles.
