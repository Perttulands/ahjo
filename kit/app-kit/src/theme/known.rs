//! The ten colour themes people expect, each taken from its published
//! specification (the 16 terminal colours, the syntax colours and the
//! interface colours as the theme assigns them) and mapped onto the
//! app's tokens by meaning: what acts, running, done, needs you, failed,
//! a gate's pass and fail. Where a theme's own colour does not read at
//! WCAG AA on its own grounds (4.5:1 for text, 3:1 for marks), its
//! lightness is moved until it does and its hue is kept; each palette
//! says which. The terminal's 16 colours are always as published. Their
//! authors and licences are in `assets/palettes`.

use super::{Direction, Palette};

/// A well-known theme's palette.
pub(super) fn palette(d: Direction) -> &'static Palette {
    match d {
        Direction::Dracula => &DRACULA,
        Direction::Nord => &NORD,
        Direction::GruvboxDark => &GRUVBOX_DARK,
        Direction::SolarizedDark => &SOLARIZED_DARK,
        Direction::SolarizedLight => &SOLARIZED_LIGHT,
        Direction::CatppuccinMocha => &CATPPUCCIN_MOCHA,
        Direction::TokyoNight => &TOKYO_NIGHT,
        Direction::OneDark => &ONE_DARK,
        Direction::Monokai => &MONOKAI,
        Direction::GithubLight => &GITHUB_LIGHT,
        Direction::Night | Direction::Saga | Direction::Signal => {
            unreachable!("the ravens' palettes are the theme's own")
        }
    }
}

/// Dracula, by Zeno Rocha and the Dracula Theme team (MIT), from its
/// specification (spec.draculatheme.com) and its terminal ports. The spec
/// has one background; the panels take its VS Code port's. Secondary text
/// and chart ink are mixes of its foreground and comment, which it lacks.
/// Lifted to read (WCAG AA): dim and comments from Comment, failed from Red.
const DRACULA: Palette = Palette {
    bg: 0x282a36,
    surface: 0x21222c,
    raised: 0x343746,
    divider: 0x44475a,
    text: 0xf8f8f2,
    text2: 0xc4c9d7,
    dim: 0x95a0c2,
    accent: 0xbd93f9,
    error: 0xff7575,
    running: 0x8be9fd,
    done: 0x50fa7b,
    needs: 0xffb86c,
    chart: 0xb4bccf,
    wire: 0x6272a4,
    pass: 0xf1fa8c,
    fail: 0xff79c6,
    term_bg: 0x282a36,
    term_fg: 0xf8f8f2,
    cursor: 0xf8f8f2,
    selection: (0x44475a, 1.0),
    ansi: [
        0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2, 0x6272a4,
        0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
    ],
    syntax: [0x8390b7, 0xf1fa8c, 0xbd93f9, 0xff79c6],
    identity: [0x8359c0, 0x337887, 0xb54585, 0x307e43],
};

/// Nord, by Sven Greb (MIT), from nordtheme.com's palette and its official
/// terminal port. Snow Storm nord6 is the text, nord4 the secondary. Nord's
/// Aurora is dark for its ground: failed (nord11), running (nord15) and the
/// comment, number and keyword colours are lifted to read, failed most.
const NORD: Palette = Palette {
    bg: 0x2e3440,
    surface: 0x343b49,
    raised: 0x3b4252,
    divider: 0x4c566a,
    text: 0xeceff4,
    text2: 0xd8dee9,
    dim: 0xa8aeb8,
    accent: 0x88c0d0,
    error: 0xd89fa4,
    running: 0xc3a5bd,
    done: 0xa3be8c,
    needs: 0xebcb8b,
    chart: 0xd8dee9,
    wire: 0x78859e,
    pass: 0x8fbcbb,
    fail: 0xd08770,
    term_bg: 0x2e3440,
    term_fg: 0xd8dee9,
    cursor: 0xd8dee9,
    selection: (0x434c5e, 1.0),
    ansi: [
        0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0, 0x4c566a,
        0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
    ],
    syntax: [0x9ba5b8, 0xa3be8c, 0xbc9ab5, 0x8aa8c5],
    identity: [0x476c91, 0x337082, 0x885b80, 0x806530],
};

/// Gruvbox Dark, medium contrast, by Pavel Pertsev (MIT/X11), from
/// gruvbox.vim: the bright accents on dark0, the terminal's neutral ones.
/// Blue acts, purple runs, yellow waits, aqua passes, orange fails a gate.
/// Lifted to read: failed and keywords (bright red), comments (gray).
const GRUVBOX_DARK: Palette = Palette {
    bg: 0x282828,
    surface: 0x32302f,
    raised: 0x3c3836,
    divider: 0x504945,
    text: 0xebdbb2,
    text2: 0xd5c4a1,
    dim: 0xaea08c,
    accent: 0x88a99c,
    error: 0xfc7b6c,
    running: 0xd58da1,
    done: 0xb8bb26,
    needs: 0xfabd2f,
    chart: 0xbdae93,
    wire: 0x928374,
    pass: 0x8ec07c,
    fail: 0xfe8019,
    term_bg: 0x282828,
    term_fg: 0xebdbb2,
    cursor: 0xebdbb2,
    selection: (0x665c54, 1.0),
    ansi: [
        0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, 0x928374,
        0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
    ],
    syntax: [0xa39689, 0xb8bb26, 0xd3869b, 0xfc6755],
    identity: [0x346567, 0x426644, 0x8d4667, 0x735a2b],
};

/// Solarized Dark, by Ethan Schoonover (MIT), from its README and
/// vim-colors-solarized. Its body text and accents sit under WCAG AA on
/// base03, so reading text is one step stronger (base2, base1 towards base2,
/// base0) and the accents are lifted in lightness, their hues kept.
const SOLARIZED_DARK: Palette = Palette {
    bg: 0x002b36,
    surface: 0x04303c,
    raised: 0x073642,
    divider: 0x2b4f59,
    text: 0xeee8d5,
    text2: 0xb7bdb6,
    dim: 0x8b9b9d,
    accent: 0x469fde,
    error: 0xe87674,
    running: 0x8f93d2,
    done: 0x8da200,
    needs: 0xc09100,
    chart: 0x839496,
    wire: 0x60787f,
    pass: 0x2aa198,
    fail: 0xd33682,
    term_bg: 0x002b36,
    term_fg: 0x839496,
    cursor: 0x839496,
    selection: (0x073642, 1.0),
    ansi: [
        0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, 0x002b36,
        0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
    ],
    syntax: [0x7d969d, 0x2aa299, 0x2aa299, 0x889c00],
    identity: [0x366a8e, 0x2a6f6a, 0xa84072, 0x76642d],
};

/// Solarized Light, by Ethan Schoonover (MIT): Solarized Dark with its
/// grounds and inks swapped, as the README sets out. As in the dark one,
/// text is a step stronger (base02, then towards base01, then base00) and
/// the accents are deepened to read on base3, their hues kept.
const SOLARIZED_LIGHT: Palette = Palette {
    bg: 0xfdf6e3,
    surface: 0xf6efdc,
    raised: 0xeee8d5,
    divider: 0xc5c8be,
    text: 0x073642,
    text2: 0x30525c,
    dim: 0x586b72,
    accent: 0x1e6da5,
    error: 0xca2522,
    running: 0x5a5fbd,
    done: 0x616f00,
    needs: 0x846400,
    chart: 0x657b83,
    wire: 0x7c8d8d,
    pass: 0x289990,
    fail: 0xd33682,
    term_bg: 0xfdf6e3,
    term_fg: 0x5f747c,
    cursor: 0x657b83,
    selection: (0xeee8d5, 1.0),
    ansi: [
        0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, 0x002b36,
        0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
    ],
    syntax: [0x617070, 0x207973, 0x207973, 0x657400],
    identity: [0x6da2c8, 0x41aca4, 0xd185aa, 0xb59a45],
};

/// Catppuccin Mocha, by Catppuccin (MIT), from its palette.json, style
/// guide and terminal port: Mauve acts, Blue runs, Yellow waits, Teal
/// passes, Pink fails a gate. It reads as published.
const CATPPUCCIN_MOCHA: Palette = Palette {
    bg: 0x1e1e2e,
    surface: 0x181825,
    raised: 0x313244,
    divider: 0x45475a,
    text: 0xcdd6f4,
    text2: 0xbac2de,
    dim: 0x949ab2,
    accent: 0xcba6f7,
    error: 0xf38ba8,
    running: 0x89b4fa,
    done: 0xa6e3a1,
    needs: 0xf9e2af,
    chart: 0xbac2de,
    wire: 0x7f849c,
    pass: 0x94e2d5,
    fail: 0xf5c2e7,
    term_bg: 0x1e1e2e,
    term_fg: 0xcdd6f4,
    cursor: 0xf5e0dc,
    selection: (0x9399b2, 0.25),
    ansi: [
        0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8, 0x585b70,
        0xf37799, 0x89d88b, 0xebd391, 0x74a8fc, 0xf2aede, 0x6bd7ca, 0xbac2de,
    ],
    syntax: [0x9399b2, 0xa6e3a1, 0xfab387, 0xcba6f7],
    identity: [0x375a92, 0x26645a, 0x923779, 0x6d5829],
};

/// Tokyo Night (night), by enkia (MIT), with the values of folke's
/// tokyonight.nvim (Apache-2.0): Blue acts, Magenta runs, Yellow waits,
/// Green1 passes, Magenta2 fails a gate. Lifted to read: the comment.
const TOKYO_NIGHT: Palette = Palette {
    bg: 0x1a1b26,
    surface: 0x16161e,
    raised: 0x292e42,
    divider: 0x3b4261,
    text: 0xc0caf5,
    text2: 0xa9b1d6,
    dim: 0x8e94b4,
    accent: 0x7aa2f7,
    error: 0xf7768e,
    running: 0xbb9af7,
    done: 0x9ece6a,
    needs: 0xe0af68,
    chart: 0xa9b1d6,
    wire: 0x5b6591,
    pass: 0x73daca,
    fail: 0xff007c,
    term_bg: 0x1a1b26,
    term_fg: 0xc0caf5,
    cursor: 0xc0caf5,
    selection: (0x283457, 1.0),
    ansi: [
        0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6, 0x414868,
        0xff899d, 0x9fe044, 0xfaba4a, 0x8db0ff, 0xc7a9ff, 0xa4daff, 0xc0caf5,
    ],
    syntax: [0x7981ab, 0x9ece6a, 0xff9e64, 0x9d7cd8],
    identity: [0x36528e, 0x2b5873, 0x633ea5, 0x694f28],
};

/// One Dark, by GitHub's Atom team (MIT), from one-dark-syntax and
/// one-dark-ui, its terminal colours from Joshua Dick's onedark.vim (MIT).
/// It has no pink, so a gate's fail is its second red (lighter apart from
/// the first, which fails). Lifted to read: the comment, a little the red.
const ONE_DARK: Palette = Palette {
    bg: 0x282c34,
    surface: 0x21252b,
    raised: 0x2c323c,
    divider: 0x3e4451,
    text: 0xabb2bf,
    text2: 0x9da5b4,
    dim: 0x9399a5,
    accent: 0x61afef,
    error: 0xe37a82,
    running: 0xc87cde,
    done: 0x98c379,
    needs: 0xe5c07b,
    chart: 0x9da5b4,
    wire: 0x6d7584,
    pass: 0x56b6c2,
    fail: 0xbf5248,
    term_bg: 0x282c34,
    term_fg: 0xabb2bf,
    cursor: 0x528bff,
    selection: (0x3e4451, 1.0),
    ansi: [
        0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf, 0x3e4452,
        0xbe5046, 0x98c379, 0xd19a66, 0x61afef, 0xc678dd, 0x56b6c2, 0x5c6370,
    ],
    syntax: [0x8c93a0, 0x98c379, 0xd19a66, 0xc678dd],
    identity: [0x244560, 0x1d484e, 0x100712, 0x120e07],
};

/// Monokai, the classic, by Wimer Hazenberg (monokai.nl, 2006): its colours
/// from the original TextMate theme, the terminal's from VS Code's built-in
/// Monokai (MIT), as Hazenberg published none. Its six accents are one short
/// of the meanings: a gate's fail mixes its pink and its purple. Lifted to
/// read: the comment, and the pink where it fails or is a keyword.
const MONOKAI: Palette = Palette {
    bg: 0x272822,
    surface: 0x1e1f1c,
    raised: 0x3e3d32,
    divider: 0x49483e,
    text: 0xf8f8f2,
    text2: 0xcac9be,
    dim: 0xaaa796,
    accent: 0x66d9ef,
    error: 0xfb7eab,
    running: 0xba93ff,
    done: 0xa6e22e,
    needs: 0xfd971f,
    chart: 0xbdbbaf,
    wire: 0x75715e,
    pass: 0xe6db74,
    fail: 0xd353b9,
    term_bg: 0x272822,
    term_fg: 0xf8f8f2,
    cursor: 0xf8f8f0,
    selection: (0x49483e, 1.0),
    ansi: [
        0x333333, 0xc4265e, 0x86b42b, 0xb3b42b, 0x6a7ec8, 0x8c6bc8, 0x56adbc, 0xe3e3dd, 0x666666,
        0xf92672, 0xa6e22e, 0xe2e22e, 0x819aff, 0xae81ff, 0x66d9ef, 0xf8f8f2,
    ],
    syntax: [0x928e78, 0xe6db74, 0xae81ff, 0xfa4989],
    identity: [0x536abe, 0x347883, 0x7f5ac1, 0x70712b],
};

/// GitHub Light (Default), by GitHub's Primer team (MIT), from
/// github-vscode-theme and Primer's colours. Secondary text is Primer's
/// gray 7, as fg.muted is the dim; its terminal ground is the inset canvas.
/// Lifted a shade to read on the raised grey: the accent and the states.
const GITHUB_LIGHT: Palette = Palette {
    bg: 0xffffff,
    surface: 0xf6f8fa,
    raised: 0xeaeef2,
    divider: 0xd0d7de,
    text: 0x1f2328,
    text2: 0x424a53,
    dim: 0x656c75,
    accent: 0x0968d8,
    error: 0xcf222e,
    running: 0x7f4cde,
    done: 0x197c36,
    needs: 0x936200,
    chart: 0x656d76,
    wire: 0x88919b,
    pass: 0x1b7c83,
    fail: 0xbf3989,
    term_bg: 0xf6f8fa,
    term_fg: 0x1f2328,
    cursor: 0x0969da,
    selection: (0x0969da, 0.2),
    ansi: [
        0x24292f, 0xcf222e, 0x116329, 0x4d2d00, 0x0969da, 0x8250df, 0x1b7c83, 0x6e7781, 0x57606a,
        0xa40e26, 0x1a7f37, 0x633c01, 0x218bff, 0xa475f9, 0x3192aa, 0x8c959f,
    ],
    syntax: [0x6b737d, 0x0a3069, 0x0550ae, 0xcf222e],
    identity: [0x6490c4, 0x3d9aa1, 0x9a7ece, 0xb18443],
};
