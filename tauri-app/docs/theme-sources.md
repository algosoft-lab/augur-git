# Built-in Theme Sources

Augur Git Tauri bundles 32 built-in themes, including 27 palettes adapted from the pinned upstream
revisions listed here. Theme colors are represented as palette data in
`src/styles/popular-theme-seeds.ts`; no upstream theme files are loaded at runtime. UI colors,
commit-graph lanes, and syntax colors are coordinated from each upstream palette. Where an upstream
project provides an ANSI palette, its colors are used for syntax and graph accents.

The theme list and initial palette data were adapted from Geared Term revision
`16f73928c0c89adc008081b32fc5993124368981`.

| Built-in themes                                   | Source and palette files                                                                                                                                                                                       | Pinned revision                            | License and attribution                  |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ | ---------------------------------------- |
| Dracula                                           | [dracula/iterm](https://github.com/dracula/iterm/tree/e5749bcd07764ad6e154038892b118ea45f88cc0/), `Dracula.itermcolors`                                                                                        | `e5749bcd07764ad6e154038892b118ea45f88cc0` | MIT; Dracula Theme                       |
| Tokyo Night, Tokyo Night Storm, Tokyo Night Light | [tokyo-night/tokyo-night-vscode-theme](https://github.com/tokyo-night/tokyo-night-vscode-theme/tree/7c0f11eaef322f293621ca7befe462214b7ea468/), `themes/tokyo-night*-color-theme.json`                         | `7c0f11eaef322f293621ca7befe462214b7ea468` | MIT; Enkia                               |
| Gruvbox Dark, Gruvbox Light                       | [morhetz/gruvbox](https://github.com/morhetz/gruvbox/tree/ef8864bb42bf244f0295d1c5a403b27e3d139695/), `colors/gruvbox.vim`                                                                                     | `ef8864bb42bf244f0295d1c5a403b27e3d139695` | MIT/X11 as declared by upstream; morhetz |
| Nord                                              | [nordtheme/nord](https://github.com/nordtheme/nord/tree/1cef71605416a222e57225b544540ce0fcec18d4/), `src/nord.styl`                                                                                            | `1cef71605416a222e57225b544540ce0fcec18d4` | MIT; Sven Greb                           |
| Solarized Dark, Solarized Light                   | [altercation/solarized](https://github.com/altercation/solarized/tree/62f656a02f93c5190a8753159e34b385588d5ff3/), `iterm2-colors-solarized/`                                                                   | `62f656a02f93c5190a8753159e34b385588d5ff3` | MIT; Ethan Schoonover                    |
| Rosé Pine, Rosé Pine Moon, Rosé Pine Dawn         | [rose-pine/neovim](https://github.com/rose-pine/neovim/tree/ff483051a47e27d84bdef47703538df1ed9f4a47/), `lua/rose-pine/palette.lua`                                                                            | `ff483051a47e27d84bdef47703538df1ed9f4a47` | MIT; Rosé Pine                           |
| Ayu Dark, Ayu Mirage, Ayu Light                   | [ayu-theme/ayu-colors](https://github.com/ayu-theme/ayu-colors/tree/e3f44fdf2a1c83e3f183d4e8acd40c6a452dcb1c/), `src/generated/dark.js`, `src/generated/mirage.js`, `src/generated/light.js`                   | `e3f44fdf2a1c83e3f183d4e8acd40c6a452dcb1c` | MIT; Konstantin Pschera                  |
| Kanagawa Wave, Kanagawa Lotus                     | [rebelot/kanagawa.nvim](https://github.com/rebelot/kanagawa.nvim/tree/bb85e4bfc8d89b0e62c8fa53ccdd13d12e2f77b3/), `lua/kanagawa/colors.lua`, `lua/kanagawa/themes.lua`, and `extras/iterm/`                    | `bb85e4bfc8d89b0e62c8fa53ccdd13d12e2f77b3` | MIT; Tommaso Laurenzi                    |
| GitHub Dark Default, GitHub Light Default         | [primer/github-vscode-theme](https://github.com/primer/github-vscode-theme/tree/cd78e5e4e7bcf132a6f428ae0f32264bb1b729cf/), `src/`                                                                             | `cd78e5e4e7bcf132a6f428ae0f32264bb1b729cf` | MIT; Primer                              |
| Atom One Dark                                     | [atom/one-dark-syntax](https://github.com/atom/one-dark-syntax/tree/9c96f4454362267ac45322063e193ccf9d2debb1/), `index.less`                                                                                   | `9c96f4454362267ac45322063e193ccf9d2debb1` | MIT; GitHub Inc.                         |
| Atom One Light                                    | [atom/one-light-syntax](https://github.com/atom/one-light-syntax/tree/d84579027410c576086dfca14d934c4bd74b0438/), `index.less`                                                                                 | `d84579027410c576086dfca14d934c4bd74b0438` | MIT; GitHub Inc.                         |
| Everforest Dark, Everforest Light                 | [sainnhe/everforest-vscode](https://github.com/sainnhe/everforest-vscode/tree/b17f8affe9096f18d209caf2fc26322490e55857/), `themes/everforest-dark.json`, `themes/everforest-light.json`                        | `b17f8affe9096f18d209caf2fc26322490e55857` | MIT; sainnhe                             |
| Night Owl, Light Owl                              | [sdras/night-owl-vscode-theme](https://github.com/sdras/night-owl-vscode-theme/tree/cc291eba7976b20d7c66bde6883c27b902196b07/), `themes/Night Owl-color-theme.json`, `themes/Night Owl-Light-color-theme.json` | `cc291eba7976b20d7c66bde6883c27b902196b07` | MIT; Sarah Drasner                       |
| Claude Dark, Claude Light                         | [mbadolato/iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes/tree/99d9701ba3cf4a06d24eea6ca4f25a64656b446b/schemes), `Claude Dark.itermcolors`, `Claude Light.itermcolors`; original [zanehu-ai/ghostty-claude-theme](https://github.com/zanehu-ai/ghostty-claude-theme/tree/ca572e2532bc34e41ce91051867b2f15cb164596), `Claude` | `99d9701ba3cf4a06d24eea6ca4f25a64656b446b`; original `ca572e2532bc34e41ce91051867b2f15cb164596` | MIT; techfitmaster and Mark Badolato |

The ANSI mappings for palettes without an upstream ANSI scheme use the semantic palette colors and,
where applicable,
[Tinted Theming schemes](https://github.com/tinted-theming/schemes/tree/50f6e3b93a8f62db9d839f8b79a709c1bbdaac53/)
(`spec-0.11`, revision `50f6e3b93a8f62db9d839f8b79a709c1bbdaac53`, MIT, © 2022 Tinted Theming) as a
palette reference. Its license is included in the bundled notice.

[iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) is useful for discovering
and comparing terminal ports, but its top-level license says individual themes retain their own
authors' copyright and licenses. The Claude pair is included with attribution to its original
MIT-licensed source and the collection; this does not treat the collection license as a blanket
license for other bundled themes. Monokai Pro is not included because its
[official license](https://monokai.pro/license) prohibits redistribution.

The full MIT notice and theme-specific attribution list ship as
`public/THIRD_PARTY_THEME_NOTICES.txt` and are included in the Tauri frontend bundle.
