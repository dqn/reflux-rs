# reflux-rs

beatmania IIDX INFINITAS のスコアトラッカー。本家 [Reflux](https://github.com/olji/Reflux) (C#) の Rust 移植版。

## プロジェクト構成

```
crates/
├── reflux-core/    # コアライブラリ（ゲームロジック、メモリ読み取り、API）
└── reflux-cli/     # CLI アプリケーション
```

## 開発コマンド

```bash
cargo build          # ビルド
cargo test           # テスト実行
cargo run            # CLI 実行（Windows のみ動作）
```

## アーキテクチャ

### reflux-core モジュール構成

| モジュール | 役割                                                         |
| ---------- | ------------------------------------------------------------ |
| `game/`    | ゲームデータ構造（PlayData, Judge, Settings, etc.）          |
| `memory/`  | Windows プロセスメモリ読み取り                               |
| `network/` | リモートサーバー API、Kamaitachi 連携                        |
| `storage/` | ローカルファイル保存（Tracker, Session, UnlockDb, ScoreMap） |
| `stream/`  | OBS 向けファイル出力                                         |
| `offset/`  | メモリオフセット検索・管理                                   |
| `config/`  | INI 設定ファイルパース                                       |
| `reflux/`  | メインアプリケーションロジック                               |
| `error.rs` | エラー型定義                                                 |

### 主要な型

- `PlayData` - プレイ結果データ
- `Judge` - 判定データ（PGreat, Great, etc.）
- `SongInfo` - 楽曲メタデータ
- `ChartInfo` - 楽曲+難易度情報
- `UnlockData` - アンロック状態
- `Tracker` - ベストスコア追跡
- `ScoreMap` - ゲーム内スコアデータ
- `OffsetsCollection` - メモリオフセット集
- `GameStateDetector` - ゲーム状態検出
- `Settings` - プレイ設定
- `Config` - INI 設定

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
- CLI は `config.ini` を読み込まず `Config::default()` 固定
- オフセットは組み込みシグネチャで検出し、`offsets.txt` は未使用
- 任意サポートファイル: `encodingfixes.txt`, `customtypes.txt`
