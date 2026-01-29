# reflux-rs

beatmania IIDX INFINITAS のスコアトラッカー。本家 [Reflux](https://github.com/olji/Reflux) (C#) の Rust 移植版。

Rust Edition 2024 を使用。

## プロジェクト構成

```
crates/
├── reflux-core/    # コアライブラリ（ゲームロジック、メモリ読み取り）
└── reflux-cli/     # CLI アプリケーション
```

## 開発コマンド

```bash
cargo build          # ビルド
cargo test           # テスト実行
cargo run            # CLI 実行（Windows のみ動作）
```

## CI/CD

GitHub Actions でビルド・リリースを自動化。

- **ci.yml**: PR/push 時に test, clippy, build-windows を実行
- **release.yml**: タグ push (`v*`) で Windows バイナリをビルドしリリース作成

## デバッグコマンド

INFINITAS のバージョン変更時にメモリ構造を調査するためのコマンド群。

### オフセット検索・状態確認

```bash
# オフセット検索（対話的）
reflux find-offsets

# ゲーム・オフセット状態表示
reflux status

# メモリ構造情報をダンプ
reflux dump
```

### メモリ分析

```bash
# メモリ構造の分析（デバッグモード）
reflux analyze

# 特定アドレスのメモリ構造探索
reflux explore --address 0x1431B08A0

# メモリの生バイトダンプ
reflux hexdump --address 0x1431B08A0 --size 256 --ascii
```

### 検索・スキャン

```bash
# メモリ検索
reflux search --string "fun"              # 文字列検索（Shift-JIS）
reflux search --i32 9003                  # 32bit整数検索
reflux search --pattern "00 04 07 0A"     # バイトパターン検索（?? でワイルドカード）

# カスタムエントリサイズでスキャン
reflux scan --entry-size 1200
```

### ユーティリティ

```bash
# アドレス間のオフセット計算
reflux offset --from 0x1431B08A0 --to 0x1431B0BD0

# 楽曲エントリ構造の検証
reflux validate song-entry --address 0x1431B08A0
```

## アーキテクチャ

### reflux-core モジュール構成

| モジュール | 役割                                             |
| ---------- | ------------------------------------------------ |
| `game/`    | ゲームデータ構造（PlayData, Judge, Settings 等） |
| `memory/`  | Windows プロセスメモリ読み取り                   |
| `storage/` | セッション管理、スコアマップ、TSV/JSON 形式      |
| `stream/`  | OBS 向けファイル出力                             |
| `offset/`  | シグネチャベースのメモリオフセット検索・管理     |
| `debug/`   | メモリダンプ、スキャン、ステータス表示           |
| `reflux/`  | メインアプリケーションロジック                   |
| `error.rs` | エラー型定義                                     |

### 主要な型

- `PlayData` - プレイ結果データ
- `Judge` - 判定データ（PGreat, Great 等）
- `SongInfo` - 楽曲メタデータ
- `Chart`, `ChartInfo` - 楽曲+難易度情報
- `UnlockData` - アンロック状態
- `Settings` - プレイ設定
- `GameStateDetector` - ゲーム状態検出
- `ScoreMap`, `ScoreData` - ゲーム内スコアデータ
- `OffsetsCollection` - メモリオフセット集
- `SessionManager` - セッション管理
- `Reflux`, `GameData` - メインアプリケーション
- `StreamOutput` - OBS 出力用ファイル生成

## 参照資料

本家 C# 実装は `.agent/Reflux/` にあり。機能追加・バグ修正時に参照。

## リリース手順

1. Cargo.toml のバージョンを更新（reflux-core, reflux-cli 両方）
2. `git tag vX.Y.Z` でタグをつける
3. `git push --tags` で push

## 注意事項

- Windows 専用（INFINITAS のメモリを読み取るため）
- macOS/Linux ではビルドは通るがメモリ読み取り機能は動作しない
- Shift-JIS エンコーディング処理あり（日本語タイトル対応）
- オフセットは組み込みシグネチャで検出し、`offsets.txt` は未使用
- 任意サポートファイル: `encodingfixes.txt`, `customtypes.txt`
