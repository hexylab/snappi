// 配布用のビルド（npm run release）。
//
// Rust はパニック時のメッセージなどのためにソースのパスを exe に埋め込むので、そのままでは
// C:\Users\<名前>\.cargo\... のようなパス（＝ユーザー名）が exe に残る。
// --remap-path-prefix でビルドした PC のパスを置き換えてから `tauri build` し、
// できた exe にホームフォルダのパスやユーザー名が残っていないことを確かめる。
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { homedir, userInfo } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const home = homedir();
const cargoHome = process.env.CARGO_HOME ?? join(home, ".cargo");
const rustupHome = process.env.RUSTUP_HOME ?? join(home, ".rustup");

// 後に書いたものが優先される。ホーム全体を最初に置いて、残りを細かく置き換える
const remaps = [
  [home, "~"],
  [cargoHome, "/cargo"],
  [rustupHome, "/rustup"],
  [root, "/snappi"],
];
const rustflags = [process.env.RUSTFLAGS ?? "", ...remaps.map(([from, to]) => `--remap-path-prefix=${from}=${to}`)]
  .join(" ")
  .trim();

// 同梱する FFmpeg が src-tauri/ffmpeg/README.txt に書いた版（gyan.dev 8.0.1 full）か確かめる。
// 版を変えたら README.txt・BUILD-INFO.txt・リリースに添付するソースも揃えること
const FFMPEG_SHA256 = "74db6c184a03dba2bdfe23e1a1f41cf5a8385bc1de6a7a1b26db1dc541abef93";
const ffmpeg = join(root, "src-tauri", "ffmpeg", "ffmpeg.exe");
if (!existsSync(ffmpeg)) {
  console.error(`✗ ${ffmpeg} がありません。src-tauri/ffmpeg/README.txt の手順で置いてください。`);
  process.exit(1);
}
if (createHash("sha256").update(readFileSync(ffmpeg)).digest("hex") !== FFMPEG_SHA256) {
  console.error("✗ ffmpeg.exe が README.txt に書いた版と違います。README.txt・BUILD-INFO.txt と FFMPEG_SHA256 を更新してください。");
  process.exit(1);
}

const build = spawnSync("npx", ["tauri", "build", ...process.argv.slice(2)], {
  cwd: root,
  stdio: "inherit",
  shell: true,
  env: { ...process.env, RUSTFLAGS: rustflags },
});
if (build.status !== 0) process.exit(build.status ?? 1);

// できた exe に、ホームフォルダのパス・ユーザー名が残っていないか確かめる（ASCII と UTF-16）
const exe = join(root, "src-tauri", "target", "release", "snappi.exe");
// 大文字・小文字は区別しない（latin1 で読むと UTF-16 は「文字\0文字\0…」になる）
const text = readFileSync(exe).toString("latin1").toLowerCase();
const needles = [home, userInfo().username].filter((s) => s.length >= 3).map((s) => s.toLowerCase());
const leaks = needles.filter((s) => text.includes(s) || text.includes([...s].join("\0")));
if (leaks.length > 0) {
  console.error(`\n✗ ${exe} にビルドした PC のパスまたはユーザー名が残っています。配布しないでください。`);
  process.exit(1);
}
console.log(`\n✓ ${exe} にホームフォルダのパス・ユーザー名は含まれていません。`);
