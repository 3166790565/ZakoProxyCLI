import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";

const noticeName = /^(licen[sc]e|copying|notice|ofl)([. -]|$)/i;
const supplements = {
  "victory-vendor": {
    file: "licenses/victory-vendor.LICENSE",
    source: "https://github.com/FormidableLabs/victory/blob/v37.3.6/LICENSE.txt",
  },
};

function noticeFiles(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory() && entry.name !== "node_modules") return noticeFiles(path);
      return entry.isFile() && noticeName.test(entry.name) ? [path] : [];
    })
    .sort();
}

export function generateThirdPartyNotices(root) {
  const lock = JSON.parse(readFileSync(join(root, "package-lock.json"), "utf8"));
  const sections = [
    "Third-Party Notices",
    "Generated from the locked, installed production dependency tree. Each dependency retains its own license; the application's MIT license does not replace these terms.",
  ];

  for (const [path, locked] of Object.entries(lock.packages).sort(([a], [b]) => a.localeCompare(b, "en"))) {
    if (!path || locked.dev) continue;
    const directory = join(root, path);
    if (!existsSync(directory) && locked.optional) continue;
    const manifest = JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));
    if (manifest.version !== locked.version) throw new Error(`Dependency version mismatch: ${path}`);
    const files = noticeFiles(directory);
    const supplement = supplements[manifest.name];
    if (!files.length && !supplement) throw new Error(`Missing license text: ${manifest.name}`);

    sections.push(`\n${"=".repeat(72)}\n${manifest.name}@${manifest.version}\nLicense: ${manifest.license ?? locked.license ?? "See notices below"}`);
    for (const file of files) {
      const contents = readFileSync(file, "utf8").trim();
      if (!contents) throw new Error(`Empty license text: ${file}`);
      sections.push(`--- ${relative(directory, file).replaceAll("\\", "/")} ---\n${contents}`);
    }
    if (supplement) {
      sections.push(`--- Supplemental upstream notice: ${supplement.source} ---\n${readFileSync(join(root, supplement.file), "utf8").trim()}`);
    }
  }
  return `${sections.join("\n\n")}\n`;
}

export function licenseNotices() {
  let root;
  return {
    name: "third-party-license-notices",
    apply: "build",
    configResolved(config) {
      root = config.root;
    },
    generateBundle() {
      this.emitFile({
        type: "asset",
        fileName: "assets/third-party-licenses.txt",
        source: generateThirdPartyNotices(root),
      });
    },
  };
}
