# HQ Git 版本发布与应用内更新

更新源：公开仓库 https://github.com/xz-he/x-git 的最新正式 GitHub Release。
客户端读取 `https://github.com/xz-he/x-git/releases/latest/download/latest.json`。
当前发布流程面向 Windows x64，生成 NSIS 安装包、`.sig` 签名和 `latest.json`。

## 首次配置（仅一次）

1. 本机已生成 `.release-keys/updater.key`（私钥）和 `.release-keys/updater.key.pub`（公钥）。目录被 Git 忽略，私钥未进入代码。妥善离线备份私钥，勿上传 Release、提交 Git 或贴入聊天。当前私钥无密码，应存入受控的密码库/密钥存储。
2. 在仓库 **Settings → Secrets and variables → Actions → New repository secret** 新增 `TAURI_SIGNING_PRIVATE_KEY`，值填写私钥文件的完整内容。当前无密码，可不配置 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`；若以后使用加密私钥，需配置对应密码。
3. `src-tauri/tauri.conf.json` 已包含对应公钥。后续版本必须使用同一私钥签名，不要每次发布重新生成；丢失私钥后旧客户端无法验证新密钥签名，只能重新手动安装或提前规划密钥迁移。
4. 已有旧 EXE 没有更新入口，需要手动安装一次本次构建的 `*-setup.exe`。自动更新以安装版为支持对象，不支持把独立 EXE 当作免安装版原地替换。

## 本机打包

```powershell
npm ci
npm run package:windows
```

脚本自动读取本机忽略目录中的私钥；也支持预先设置 `TAURI_SIGNING_PRIVATE_KEY`（文件路径或完整内容）及可选密码。安装包位于 `src-tauri/target/release/bundle/nsis/`。
直接运行 `npm run tauri -- build` 时，需要自行设置上述签名环境变量。
更新签名用于应用内验签，不是 Windows Authenticode 证书，因此不会消除 Windows SmartScreen 提示。

## 发布后续版本

1. 修改代码并同步版本号（必须递增）：

   ```powershell
   npm run release:version -- 4.0.1
   npm run release:version -- --check
   npm test -- --maxWorkers=2
   npm run build
   ```

   脚本同步 `package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`。首次发布也可以使用当前 4.0.0，无需先递增。

2. 审查并提交代码及版本文件，推送到仓库。为该提交创建相同版本的标签并推送，例如：

   ```powershell
   git tag v4.0.1
   git push origin v4.0.1
   ```

3. 标签触发 `.github/workflows/release.yml`：验证版本一致性，执行测试，构建已签名的安装包，并创建 **Draft Release**。任务失败时先查看 GitHub Actions 日志；不要发布不完整的 Release。
4. 在 GitHub Releases 打开草稿，确认含 `*-setup.exe`、对应 `.sig` 和 `latest.json`，并核对 JSON 的版本、签名、`windows-x86_64` 平台及版本固定的下载 URL。补充更新说明后点击 **Publish release**，作为最新正式版。草稿或预发布版不会进入当前稳定更新渠道。
5. 在已安装的较低版本中，打开 **设置 → 版本更新 → Check for Updates**，核对版本与说明，下载，确认 **Install & Restart**。测试旧设置保留、重启后版本正确。相同版本不会提示更新。

`latest.json` 中的说明由打包动作生成；若编辑了 Release 正文，需同步修改该资产中的 `notes` 才会改变应用内说明。不要修改签名或安装包地址，不要在发布后替换为未签名安装包。

## 客户端行为与故障排查

- 默认每次启动正式安装版时检查一次，可在设置关闭。开发模式不自动联网；手动检查仍可使用。
- 只自动检查，不自动下载或安装。发现更新后轻提示；无新版本或后台检查失败时不打断操作，错误可在更新设置查看。
- 下载显示实际字节数，有总大小时显示百分比；完成下载且签名校验通过才允许安装。关闭设置窗口不会取消下载。
- 安装前需用户确认。Git / AI 任务、设置保存、未保存的冲突解决草稿会阻止安装。其他尚未保存的输入需用户提前保存。Windows 安装器会关闭并重启应用。
- 尚无正式 Release 或缺少 `latest.json` 时，检查会提示失败，不会误报“已是最新”。网络阻断 GitHub 时可重试或手动下载安装包。
- 签名失败：确认签名私钥与客户端内置公钥配对、安装包没有被修改。不能绕过验签安装。

本次代码实现与本机测试不等于线上发布完成；配置 Actions secret、推送发布标签、审核并发布草稿后，更新服务才会向客户端提供版本。
