"""Import inert prompt data from the pinned Coldcoffee source checkout.

Usage: python scripts/import-coldcoffee.py PATH_TO_GPT6_ASTRA_CHECKOUT
No upstream JavaScript, installer, Skill, or prompt is executed.
"""

import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

REVISION = "09c7799a1fc2ef5f6f6cf4223b7a48abb39d2732"
MODELS = {
    "claude": [
        "claude-fable-5", "claude-fable-5-1", "claude-haiku-4-5", "claude-haiku-5-5",
        "claude-opus-4-5", "claude-opus-4-6", "claude-opus-4-7", "claude-opus-4-8",
        "claude-opus-5", "claude-opus-5-5", "claude-sonnet-4-5", "claude-sonnet-4-6",
        "claude-sonnet-5", "claude-sonnet-5-5",
    ],
    "grok": ["grok-4.7"],
    "gemini": [
        "gemini-3-flash", "gemini-3.1-flash-image", "gemini-3.1-pro",
        "gemini-3.1-pro-preview", "gemini-3.6-flash", "gemini-3.7-flash", "gemini-3.8-flash",
    ],
    "deepseek": [
        "deepseek-v4-flash", "deepseek-v4-flash-0731", "deepseek-v4-pro",
        "deepseek-v4-pro-0813", "deepseek-v4.1-flash",
    ],
    "glm53": ["glm-5.3"],
}
ROUTES = ["cha-bin-unlock", "cha-playfield", "cha-netcast", "cha-apicloud", "cha-labpipe", "cha-inkstage"]
ADAPTER = """# ZakoAI · 冷咖啡 API 内联适配

以下席位、工作链和路由技能正文由代理直接随本次请求提供。读取这些正文即可，
不需要再读取本机 SKILL.md 或调用 Harness skill 加载器。正文的技能名是内容标签，
不代表已经执行过工具或安装过客户端文件。只使用当前请求实际提供的 tools，
工具、终端、文件系统、子代理和 IDA/MCP 的可用性以实际客户端为准。
保留客户端的消息历史、工具调用关联、图片及其他输入，按用户要求交付结果。
仅把真实工具返回作为已执行证据，不编造文件、权限、会话持久化或工具结果。

"""


def import_checkout(source: Path, root: Path):
    actual = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    if actual != REVISION:
        raise ValueError(f"Expected upstream {REVISION}, got {actual}")
    if subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True).strip():
        raise ValueError("Upstream checkout must be clean")
    packs = source / "desktop/src/lib/packs"
    inputs = {}

    def read(relative):
        path = packs / relative
        # Read canonical Git blobs so imports/hashes do not depend on core.autocrlf.
        data = subprocess.check_output(["git", "-C", str(source), "show", f"{REVISION}:{path.relative_to(source).as_posix()}"])
        inputs[path.relative_to(source).as_posix()] = hashlib.sha256(data).hexdigest()
        return data.decode("utf-8").rstrip()

    router = "\n\n".join([
        read("routes/ROUTER.md"), read("routes/NORMALIZER.md"),
        *(read(f"routes/{route}.md") for route in ROUTES),
        read("routes/leaves/INDEX.md"),
        *(read(f"routes/leaves/{path.name}") for path in sorted((packs / "routes/leaves").glob("*.md")) if path.name != "INDEX.md"),
    ])
    target = root / "profiles/coldcoffee"
    target.mkdir(parents=True, exist_ok=True)
    catalog = []
    outputs = {}
    for seat, models in MODELS.items():
        text = read(f"{seat}.md")
        variables = {
            "APP_TITLE": "ZakoAI · 冷咖啡策略", "ACTIVATION_WORD": "冷咖啡",
            "CONTROL_WORD": "BREAK//OPEN", "SEAT_ID": seat, "SEAT_TAG": seat,
            "SEAT_NAME": seat, "ACTIVATION_REPLY": read("activation-reply.md"),
            "WORKFLOW": read(f"workflows/{seat}.md"), "ROUTER": router,
            "IDA_MCP": "", "UNIFIED": "",
        }
        for key, value in variables.items():
            text = text.replace("{{" + key + "}}", value)
        if re.search(r"\{\{[A-Z_]+\}\}", text):
            raise ValueError(f"Unresolved template in {seat}")
        text = ADAPTER + text.rstrip() + "\n"
        data = text.encode("utf-8")
        (target / f"{seat}.md").write_bytes(data)
        outputs[seat] = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
        for model in models:
            catalog.append({"id": f"coldcoffee-{model}-v1", "name": f"冷咖啡 · {model}", "models": [model], "model_family": "glm" if seat == "glm53" else seat})
    (target / "index.json").write_bytes((json.dumps(catalog, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
    provenance = {
        "repository": "https://github.com/3641397194-wq/gpt6-Astra", "revision": REVISION,
        "license": "MIT", "copyright": "Copyright (c) 2026 3641397194-wq",
        "adaptations": ["Inline seat, workflow, router and leaf Skill text as request data", "Add truthful API runtime guidance instead of claiming a Harness tool ran", "Omit installer-specific IDA/MCP configuration and commercial unified-work block", "Keep exact user-specified model IDs; remove pasted svg separators"],
        "inputs": inputs, "outputs": outputs,
    }
    (target / "SOURCE.json").write_bytes((json.dumps(provenance, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
    license_text = subprocess.check_output(["git", "-C", str(source), "show", f"{REVISION}:LICENSE"])
    (root / "third_party/coldcoffee.LICENSE").write_bytes(license_text)
    print(f"Imported {len(catalog)} exact-model profiles from {REVISION}")
    for seat, result in outputs.items():
        print(f"{seat}: {result['bytes']} bytes, SHA256 {result['sha256']}")


if __name__ == "__main__":
    import_checkout(Path(sys.argv[1]).resolve(), Path(__file__).resolve().parents[1])
