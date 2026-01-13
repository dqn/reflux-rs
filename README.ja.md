# Reflux-RS

[Reflux](https://github.com/olji/Reflux) の Rust 再実装。beatmania IIDX INFINITAS 用スコアトラッカー。

[English version](README.md)

## 機能

- **メモリ読み取り**: INFINITAS プロセスから直接ゲームデータを読み取り
- **スコア記録**: 判定、スコア、クリアランプを含むプレイ結果を記録
- **オフセット自動検索**: ゲームアップデート時にメモリオフセットを自動検出
- **ローカル保存**: TSV/JSON ファイルにスコアを保存
- **リモート同期**: リモートサーバにスコアを同期
- **Kamaitachi 連携**: [Kamaitachi](https://kamai.tachi.ac/) へのスコア送信に対応
- **OBS 配信対応**: 現在の楽曲情報やプレイ状態をテキストファイルに出力

## 動作要件

- Windows（ReadProcessMemory API を使用）
- Rust 1.85+（Edition 2024）
- beatmania IIDX INFINITAS

## インストール

### ソースからビルド

```bash
git clone https://github.com/dqn/reflux-rs.git
cd reflux-rs
cargo build --release
```

バイナリは `target/release/reflux.exe` に生成されます。

## 使い方

```bash
# デフォルト設定で実行
reflux

# 設定ファイルとオフセットファイルを指定
reflux --config config.ini --offsets offsets.txt

# ヘルプを表示
reflux --help
```

### コマンドラインオプション

| オプション | デフォルト | 説明 |
|-----------|-----------|------|
| `-c, --config` | `config.ini` | 設定ファイルのパス |
| `-o, --offsets` | `offsets.txt` | オフセットファイルのパス |
| `-t, --tracker` | `tracker.db` | トラッカーデータベースのパス |

## 設定

`config.ini` ファイルを作成:

```ini
[Update]
updatefiles = true
updateserver = https://example.com

[Record]
saveremote = false
savelocal = true
savejson = true
savelatestjson = false
savelatesttxt = true

[RemoteRecord]
serveraddress = https://example.com
apikey = your-api-key

[LocalRecord]
songinfo = true
chartdetails = true
resultdetails = true
judge = true
settings = true

[Livestream]
playstate = true
marquee = true
fullsonginfo = false
marqueeidletext = INFINITAS

[Debug]
outputdb = false
```

## オフセットファイル

`offsets.txt` ファイルにはゲームデータを読み取るためのメモリアドレスが含まれます:

```
P2D:J:B:A:2025010100
songList = 0x12345678
dataMap = 0x12345678
judgeData = 0x12345678
playData = 0x12345678
playSettings = 0x12345678
unlockData = 0x12345678
currentSong = 0x12345678
```

ゲームがアップデートされてオフセットが変わった場合、Reflux は自動的に新しいオフセットを検索します。

## プロジェクト構成

```
reflux-rs/
├── Cargo.toml              # ワークスペース設定
├── crates/
│   ├── reflux-core/        # コアライブラリ
│   │   └── src/
│   │       ├── config/     # INI 設定パーサー
│   │       ├── game/       # ゲームデータ構造
│   │       ├── memory/     # Windows API ラッパー
│   │       ├── network/    # HTTP クライアント、Kamaitachi API
│   │       ├── offset/     # オフセット管理
│   │       ├── storage/    # ローカル永続化
│   │       ├── stream/     # OBS 配信出力
│   │       └── reflux.rs   # メイントラッカーロジック
│   │
│   └── reflux-cli/         # CLI アプリケーション
│       └── src/main.rs
```

## 出力ファイル

### セッションファイル

プレイデータは `sessions/Session_YYYY_MM_DD_HH_MM_SS.tsv` に保存されます。

### 配信用ファイル（OBS 用）

| ファイル | 説明 |
|---------|------|
| `playstate.txt` | 現在の状態: `menu`、`play`、`off` |
| `marquee.txt` | 現在の楽曲タイトルまたはステータス |
| `latest.txt` | 最新のプレイ結果 |
| `latest-grade.txt` | 最新のグレード（AAA、AA など） |
| `latest-lamp.txt` | 最新のクリアランプ |

## 開発

```bash
# ビルド
cargo build

# テスト実行
cargo test

# ログ付きで実行
RUST_LOG=reflux=debug cargo run

# コード品質チェック
cargo clippy
```

## ライセンス

MIT License - 詳細は [LICENSE](LICENSE) を参照。

## クレジット

- オリジナルの [Reflux](https://github.com/olji/Reflux) by olji
- スコアトラッキングプラットフォーム [Kamaitachi/Tachi](https://github.com/zkrising/Tachi)
