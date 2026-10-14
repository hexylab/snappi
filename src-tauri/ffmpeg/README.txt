FFmpeg について / About FFmpeg
==============================

Snappi は動画の書き出しと音声のミックスに FFmpeg を使います。このフォルダの ffmpeg.exe は
Snappi とは別のプログラムで、Snappi は別プロセスとして起動するだけです（リンクはしていません）。

Snappi uses FFmpeg to encode videos and mix audio. ffmpeg.exe in this folder is a separate
program that Snappi runs as a child process; it is not linked into Snappi.

- バージョン / Version : FFmpeg 8.0.1 (8.0.1-full_build-www.gyan.dev)
- ビルド / Build       : https://www.gyan.dev/ffmpeg/builds/
- ライセンス / License : GNU General Public License version 3（COPYING.GPLv3.txt）
- ソース / Source      : https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz
                         https://github.com/FFmpeg/FFmpeg/commit/894da5ca7d
                         Snappi の各リリースにも同じソースを添付しています。
                         The same source archive is attached to each Snappi release:
                         https://github.com/hexylab/snappi/releases
- 構成と同梱ライブラリ / Configuration and bundled libraries : BUILD-INFO.txt

FFmpeg is a trademark of Fabrice Bellard, originator of the FFmpeg project.

開発者向け: ビルドする前に、上記のビルドの bin\ffmpeg.exe をこのフォルダに置いてください
（ffmpeg.exe は Git に含めていません）。
For developers: put bin\ffmpeg.exe from the build above into this folder before building
(ffmpeg.exe is not committed to Git).
