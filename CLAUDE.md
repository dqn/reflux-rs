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

## データエクスポート

全曲のプレイデータ（スコア、ランプ、ミスカウント、DJ ポイント等）をエクスポートする。

```bash
# TSV形式でファイルに出力（デフォルト）
reflux export -o scores.tsv

# JSON形式でファイルに出力
reflux export -o scores.json -f json

# 標準出力にTSV出力
reflux export

# 標準出力にJSON出力
reflux export -f json
```

### オプション

| オプション          | 説明                                   |
| ------------------- | -------------------------------------- |
| `-o, --output`      | 出力ファイルパス（省略時は標準出力）   |
| `-f, --format`      | 出力形式: `tsv`（デフォルト）/ `json`  |
| `--pid`             | プロセスID（省略時は自動検出）         |

## アーキテクチャ

### reflux-core モジュール構成

| モジュール         | 役割                                               |
| ------------------ | -------------------------------------------------- |
| `chart/`           | 楽曲・譜面データ構造                               |
| `play/`            | ゲームプレイデータ（PlayData, Judge, Settings 等） |
| `process/`         | Windows プロセスメモリ読み取り                     |
| `score/`           | スコアデータ管理                                   |
| `session/`         | セッション管理、TSV/JSON 形式                      |
| `export/`          | データエクスポート（ExportFormat trait）           |
| `offset/`          | メモリオフセット検索・管理                         |
| `offset/searcher/` | オフセット検索のサブモジュール群                   |
| `debug/`           | メモリダンプ、スキャン、ステータス表示（要 feature） |
| `reflux/`          | メインアプリケーションロジック                     |
| `error.rs`         | エラー型定義                                       |

### offset/searcher サブモジュール

| サブモジュール       | 役割                                       |
| -------------------- | ------------------------------------------ |
| `core.rs`            | OffsetSearcher 構造体と基本操作            |
| `song_list.rs`       | SongList 検索ロジック                      |
| `relative_search.rs` | 相対オフセット検索                         |
| `data_map.rs`        | DataMap/UnlockData 検索・検証              |
| `buffer.rs`          | バッファ管理とパターン検索ヘルパー         |
| `interactive.rs`     | 対話的オフセット検索ワークフロー           |
| `validation/`        | オフセット候補のバリデーション関数         |
| `pattern.rs`         | パターン検索ユーティリティ（memchr 使用）  |
| `constants.rs`       | 検索関連の定数                             |
| `types.rs`           | 検索結果の型定義                           |
| `legacy.rs`          | レガシーシグネチャ検索（feature-gated）    |

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
- `OffsetSearcher`, `OffsetSearcherBuilder` - オフセット検索（Builder パターン対応）
- `SessionManager` - セッション管理
- `Reflux`, `RefluxConfig`, `GameData` - メインアプリケーション（設定外部化対応）
- `ExportFormat`, `TsvExporter`, `JsonExporter` - エクスポート形式（trait ベース）

### Feature Flags

| Feature             | 説明                                               |
| ------------------- | -------------------------------------------------- |
| `debug-tools`       | debug モジュールを有効化（CLI 用、本番向けでない） |
| `legacy-signatures` | レガシーシグネチャ検索コードを有効化               |

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
- オフセットは相対検索で検出（シグネチャ検索は無効化、後述）
- `offsets.txt` は未使用
- **このファイルは実装と同期して最新の状態を保つこと**

## オフセット検索の仕組み

### 検索戦略

オフセット検索は**相対オフセット検索**を主軸としている：

1. **SongList**: パターン検索（`"5.1.1."` バージョン文字列）でアンカーを取得
   - 期待位置 `base + 0x3180000` から検索開始（高速化のため）
2. **JudgeData**: SongList からの相対オフセット（-0x94E3C8）で検索
   - **Cross-validation**: 推論された CurrentSong 位置も検証
3. **PlaySettings**: JudgeData からの相対オフセット（-0x2ACFA8）で検索
   - **Cross-validation**: 推論された PlayData 位置も検証
4. **PlayData**: PlaySettings からの相対オフセット（+0x2A0）で検索
5. **CurrentSong**: JudgeData からの相対オフセット（+0x1E4）で検索
6. **DataMap/UnlockData**: パターン検索

### シグネチャ検索の無効化

シグネチャ（AOB）検索は **Version 2 (2026012800) で完全に機能しなくなった**ため無効化した：

| シグネチャ | Version 2 での検索結果 |
|-----------|----------------------|
| judgeData | 0件 |
| playSettings | 0件 |
| currentSong | 0件 |

コードは将来のために残しているが、デフォルトでは使用しない。

### 相対オフセットの定数値

バージョン間での相対オフセット差分（Version 1 → Version 2）：

| 関係 | Version 1 | Version 2 | 定数値 | 検索範囲 |
|------|-----------|-----------|--------|---------|
| SongList - JudgeData | 0x94E374 | 0x94E4B4 | 0x94E3C8 | ±64KB |
| JudgeData - PlaySettings | 0x2ACEE8 | 0x2ACFA8 | **0x2ACFA8** | ±512B |
| PlayData - PlaySettings | 0x2C0 | 0x2A0 | **0x2A0** | ±256B |
| CurrentSong - JudgeData | 0x1E4 | 0x1E4 | 0x1E4 | ±256B |

**注意**: 定数値は Version 2 に合わせて更新済み。検索範囲を狭めることで誤検出を防止。

### バリデーション戦略

オフセット検出の信頼性を高めるため、以下のバリデーションを実施：

1. **JudgeData**: 判定データ領域（72バイト）が all zeros または妥当な範囲内
2. **PlaySettings**: 設定値が有効範囲内 + song_select_marker チェック
3. **PlayData**: song_id が 1000-50000 の範囲内（all zeros は拒否）
4. **CurrentSong**: song_id が有効範囲内 + 2のべき乗を除外（all zeros は拒否）
5. **Cross-validation**: 関連オフセット同士の整合性を検証

### 新バージョン対応時

1. `cargo run --features debug-tools -- status` でオフセット検出状態を確認
2. 検出されたオフセットと `.agent/offsets-*.txt` の期待値を比較
3. 差分が検索範囲を超える場合は `constants.rs` の定数を更新
4. バリデーションが誤検出を起こす場合は検索範囲を狭める

### 過去の教訓

- **検索範囲は狭い方が安全**: 広い検索範囲は誤検出の原因になる
- **Cross-validation が重要**: 単体のバリデーションは弱いため、関連オフセット同士の整合性をチェック
- **all zeros の許容は危険**: 間違ったアドレスでも zeros が入っている可能性があるため、オフセット検索時は拒否する
- **定数値はバージョンごとに検証**: 新バージョン対応時は必ず実際の値と比較して更新
