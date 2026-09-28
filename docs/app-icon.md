# 应用图标

采用用户选定的第 03 版“知识方块”：紫色与蓝色文件夹、折页组成的立方体。按用户要求移除白色圆角底板，图案周围及各部分之间的空隙使用透明背景。源文件为根目录的 `app-icon.png`，由内置 imagegen 生成并编辑；不依赖外部路径或运行时图片服务。

源图保留紫蓝色图案及折页细节，Windows、macOS 和应用内图标使用 PNG 的透明通道。

## 使用位置

- `src-tauri/icons/`：Tauri 生成的 Windows、macOS 及其他平台资源；窗口和托盘使用默认应用图标。
- `public/app-icon.png`：256 像素 PNG，用于应用内标识及浏览器图标。

重新生成：

```sh
npm run tauri icon app-icon.png
```

随后将 `src-tauri/icons/128x128@2x.png` 复制到 `public/app-icon.png`。原有的 `app-icon.svg` 已被 PNG 源图替代。

## 去除底板的编辑提示词

Built-in imagegen; transparent background.

> Use case: background-extraction. Edit target: the supplied CPAH Docs icon. REMOVE THE ENTIRE IVORY/WHITE ROUNDED-SQUARE BACKING TILE. The user wants ONLY the purple and periwinkle-blue 3D document/folder cube floating on true alpha transparency. Preserve all three parts of the existing colored symbol: the lavender folded sheet on top, the purple layered folders on the left, and the blue folders on the right. Preserve their colors, shapes, lighting, relative positions, proportions, and fine folds exactly. Delete all white and ivory background areas, including the white spaces seen between these separate parts of the cube. Delete the backing tile completely, its soft gray cast shadows, and all stray background speckles. Transparent pixels must surround the symbol and fill every gap between its parts. Keep light highlights belonging to the colored paper material; do not cut holes into the actual purple/blue objects. Smooth clean anti-aliased silhouette, no white halo, no border, no badge, no replacement background, no checkerboard baked into the pixels. Keep a square canvas and balanced transparent padding, centered symbol, ready for a desktop app icon. This is a background removal only, not a redesign.

## 原始生成提示词

Built-in imagegen; transparent background.

> Use case: logo-brand. Asset type: one square desktop application icon concept for CPAH Docs, a local app that converts documents to Markdown, organizes folders, labels documents, and builds knowledge indexes. Create a polished original icon with a strong simple silhouette readable at 24 px. Frontal straight-on, centered, almost fills the canvas with generous internal padding. Rounded-square app tile. Transparent canvas outside tile. No text, no letters, no watermarks, no mockup scene, no tiny details, no command key symbol. Crisp vector-like geometry, restrained dimensional finish. Direction 3: sculptural violet and periwinkle interlocking paper planes form a compact open cube, representing documents becoming structured knowledge. Ivory rounded-square background, sophisticated soft 3D paper material, clear large planes, isometric mark centered within frontal tile.
