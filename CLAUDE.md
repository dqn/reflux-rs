# Reflux-RS

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

| モジュール | 役割 |
|-----------|------|
| `game/` | ゲームデータ構造（PlayData, Judge, Settings, etc.） |
| `memory/` | Windows プロセスメモリ読み取り |
| `network/` | リモートサーバー API、Kamaitachi 連携 |
| `storage/` | ローカルファイル保存（Tracker, Session, UnlockDb） |
| `stream/` | OBS 向けファイル出力 |
| `offset/` | メモリオフセット検索・管理 |
| `config/` | INI 設定ファイルパース |
| `reflux.rs` | メインアプリケーションロジック |

### 主要な型

- `PlayData` - プレイ結果データ
- `Judge` - 判定データ（PGreat, Great, etc.）
- `SongInfo` - 楽曲メタデータ
- `UnlockData` - アンロック状態
- `Tracker` - ベストスコア追跡
- `OffsetsCollection` - メモリオフセット集

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
