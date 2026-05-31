# Desktop Pet 🐾

Windows デスクトップ上に常駐するキャラクターアプリです。Tauri v2 で構築されています。

## 機能

- **デスクトップペット**: 透明・枠なし・常に前面のウィンドウにキャラクターが表示され、ふよふよ浮遊します
- **設定画面**: キャラクターをクリックすると設定画面が開きます
- **アラーム**: 設定した時刻にキャラクターが揺れてアラーム音が鳴ります。クリックで停止
- **ドラッグ移動**: キャラクターをドラッグして好きな位置に移動できます

## 将来の機能

- 天気・カレンダー表示
- ローカルLLMとの会話
- SwitchBot/Nature Remo連携による家電操作（ライト・テレビ等）
- キャラクターの感情・アニメーション

## 開発環境

| 項目 | 要件 |
|------|------|
| OS | Windows 10 / 11（開発・実行） |
| Node.js | 18+ |
| Rust | 1.70+ |
| VS Build Tools | MSVC + Windows SDK |

## セットアップ

```bash
npm install
npm run tauri dev
```

## ビルド

```bash
npm run tauri build
```

`src-tauri/target/release/bundle/` に `.msi` / `.exe` が生成されます。

## ディレクトリ構成

```
desktop-pet/
├── index.html          # キャラクターウィンドウ
├── settings.html       # 設定画面
├── src/
│   ├── main.ts         # キャラクター描画・アラーム・ドラッグ
│   └── settings.ts     # 設定画面のロジック
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json # ウィンドウ設定（透明・枠なし・常に前面）
│   └── src/
│       ├── main.rs
│       └── lib.rs      # アラーム管理・マルチウィンドウ
└── vite.config.ts
```
