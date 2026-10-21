# Astralink 图标

图形采用「星核 + 连接轨道」：四角星表示星枢，单条倾斜轨道与两端节点表示连接汇聚。桌面版本使用深海蓝圆角底、薄荷青轨道和浅色星核；界面版本使用相同轮廓，跟随当前主题色。

## 源稿与生成

- `src/assets/astralink-mark.svg`：64 × 64 矢量图形母版，界面与桌面图标共用。
- `scripts/generate-icons.mjs`：添加桌面底板，再调用项目安装的 Tauri CLI 输出平台图标。
- `src/assets/astralink-icon.svg`：生成的 512 × 512 完整图标，也是浏览器 favicon。
- `src-tauri/icons/`：打包使用的 ICO、ICNS 和 32、64、128、256、512 像素 PNG。

修改图形母版或脚本中的底板样式后，手动运行：

```powershell
node scripts/generate-icons.mjs
```

图标生成不挂到 build/test 钩子，不增加运行依赖。Tauri 的 `bundle.icon` 引用平台资源；Windows NSIS 的 `installerIcon` 与 `uninstallerIcon` 显式引用同一 ICO。窗口和托盘继续使用应用默认图标。`windows/installer-hooks.nsh` 在安装完成后发送 Shell 图标与快捷方式更新通知，处理覆盖安装后 Explorer 仍显示旧图的情况，无需重启资源管理器。

`NexusMark.vue` 使用 SVG mask 保留主题色。导入使用 `?no-inline`，确保 Vite 产物提供独立 SVG 地址，避免内联 SVG 中的括号使 CSS 的 URL 失效。

重新生成可分发的 Windows 安装包：

```powershell
npm run desktop:build
```

输出位于 `src-tauri/target/release/bundle/nsis/Astralink_0.1.0_x64-setup.exe`。macOS 与 Linux 图标资源已生成，平台安装包需在对应构建环境验证。
