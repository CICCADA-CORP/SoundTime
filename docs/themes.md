# Creating Themes for SoundTime

> SoundTime supports custom CSS themes that can be installed from git repositories.
> Themes override the default appearance by providing CSS files and static assets (images, fonts).

## Table of Contents

1. [Quick Start](#quick-start)
2. [Theme Structure](#theme-structure)
3. [Manifest File (`theme.toml`)](#manifest-file-themetoml)
4. [CSS Variables Reference](#css-variables-reference)
5. [Allowed Files](#allowed-files)
6. [Security Constraints](#security-constraints)
7. [Safe Mode](#safe-mode)
8. [Testing Locally](#testing-locally)
9. [Publishing](#publishing)
10. [API Reference](#api-reference)

---

## Quick Start

### 1. Create a theme repository

Create a new directory for your theme with two required files — a manifest and a CSS file:

```
my-theme/
├── theme.toml
└── theme.css
```

### 2. Write the manifest

Create `theme.toml` with your theme metadata:

```toml
[theme]
name = "my-theme"
version = "1.0.0"
description = "My first SoundTime theme"
author = "Your Name"

[assets]
css = "theme.css"
```

### 3. Write your CSS

Create `theme.css` that overrides SoundTime's CSS custom properties. Here is a complete example theme called **Neo Dark** that swaps the default green accent for purple:

```css
/* Neo Dark — SoundTime Theme */
/* Override CSS custom properties to change the color scheme */

:root {
  --background: 240 10% 4%;
  --foreground: 0 0% 95%;
  --card: 240 10% 7%;
  --card-foreground: 0 0% 95%;
  --popover: 240 10% 7%;
  --popover-foreground: 0 0% 95%;
  --primary: 262 83% 58%;
  --primary-foreground: 0 0% 100%;
  --secondary: 240 10% 12%;
  --secondary-foreground: 0 0% 95%;
  --muted: 240 10% 12%;
  --muted-foreground: 240 5% 55%;
  --accent: 262 83% 58%;
  --accent-foreground: 0 0% 100%;
  --destructive: 0 84% 60%;
  --destructive-foreground: 0 0% 100%;
  --border: 240 10% 15%;
  --input: 240 10% 15%;
  --ring: 262 83% 58%;
  --radius: 0.75rem;
}
```

### 4. Push to a git repository

```sh
cd my-theme
git init
git add .
git commit -m "Initial theme"
git remote add origin https://github.com/yourname/soundtime-theme-neo-dark.git
git push -u origin main
```

### 5. Install via the admin panel

1. Log in to your SoundTime instance as an admin.
2. Navigate to **Settings → Themes**.
3. Paste your repository's HTTPS URL (e.g. `https://github.com/yourname/soundtime-theme-neo-dark.git`).
4. Click **Install**.
5. Click **Enable** on the newly installed theme.

The theme CSS is applied immediately to all users.

---

## Theme Structure

A theme is a git repository with the following layout:

```
my-theme/
├── theme.toml          # Required — theme manifest
├── theme.css           # Required — main CSS file
└── assets/             # Optional — static assets
    ├── fonts/
    │   └── custom.woff2
    └── images/
        └── logo.svg
```

| Path | Required | Description |
|------|----------|-------------|
| `theme.toml` | Yes | Manifest file that identifies the theme, its version, and file paths. Must be at the repository root. |
| `theme.css` | Yes | The main CSS file. Path is declared in `theme.toml` under `[assets] css`. |
| `assets/` | No | Directory for static assets (fonts, images). Path is declared in `theme.toml` under `[assets] assets_dir`. |

The CSS file and assets directory can be named anything and placed anywhere in the repository — their locations are declared in the manifest. The examples in this guide use `theme.css` and `assets/` by convention.

---

## Manifest File (`theme.toml`)

The manifest is a [TOML](https://toml.io/) file at the root of the repository. It has two sections: `[theme]` and `[assets]`.

### Full specification

```toml
[theme]
name = "my-theme"              # Required
version = "1.0.0"              # Required
description = "A cool theme"   # Optional
author = "Your Name"           # Optional
license = "MIT"                # Optional
homepage = "https://example.com/my-theme"  # Optional

[assets]
css = "theme.css"              # Required
assets_dir = "assets"          # Optional
```

### Field reference

#### `[theme]` section

| Field | Required | Type | Constraints | Description |
|-------|----------|------|-------------|-------------|
| `name` | Yes | String | Must match `^[a-z][a-z0-9-]{1,63}$` | Unique identifier for the theme. Lowercase letters, digits, and hyphens only. Must start with a letter. Maximum 64 characters. |
| `version` | Yes | String | [Semantic versioning](https://semver.org/) (e.g. `1.0.0`, `2.1.0-beta.1`) | Used to detect updates. Bump this when you release changes. |
| `description` | No | String | Max 500 characters | Short description shown in the admin panel. |
| `author` | No | String | Max 255 characters | Author name or organization. |
| `license` | No | String | Max 50 characters | SPDX license identifier (e.g. `MIT`, `GPL-3.0-only`, `AGPL-3.0-or-later`). |
| `homepage` | No | String | Valid HTTPS URL, max 500 characters | Link to the theme's homepage, documentation, or repository. |

#### `[assets]` section

| Field | Required | Type | Constraints | Description |
|-------|----------|------|-------------|-------------|
| `css` | Yes | String | Relative path, no `..`, must end in `.css` | Path to the main CSS file, relative to the repository root. |
| `assets_dir` | No | String | Relative path, no `..` | Path to the static assets directory, relative to the repository root. Files in this directory are served at `/api/themes/assets/`. |

### Naming rules

The `name` field is the primary identifier used to prevent duplicate installations and to reference the theme in URLs and the database. It must:

- Start with a lowercase letter (`a`–`z`)
- Contain only lowercase letters, digits (`0`–`9`), and hyphens (`-`)
- Be between 2 and 64 characters long
- Not start or end with a hyphen

Valid examples: `neo-dark`, `synthwave80s`, `minimal-light`, `my-theme-v2`

Invalid examples: `My Theme` (spaces, uppercase), `-broken` (leading hyphen), `a` (too short), `123theme` (starts with digit)

---

## CSS Variables Reference

SoundTime's UI is built with [Tailwind CSS](https://tailwindcss.com/) and [shadcn-svelte](https://www.shadcn-svelte.com/). All colors and the border radius are controlled through CSS custom properties defined on `:root`. Themes override these variables to change the entire application's appearance.

### HSL format

Variables use **raw HSL components** — three space-separated numbers representing hue, saturation, and lightness — **without** the `hsl()` wrapper. The components consume them with `hsl(var(--name))`:

```css
/* In your theme CSS: */
:root {
  --primary: 262 83% 58%;
}

/* How the app uses it (you do NOT write this): */
.button {
  background-color: hsl(var(--primary));
}
```

This allows the UI to add opacity modifiers like `hsl(var(--primary) / 0.5)` without needing to restructure the variable.

### Default values

These are SoundTime's built-in defaults. Your theme overrides whichever variables you want to change:

```css
:root {
  /* ── Backgrounds ─────────────────────────────────── */
  --background: 0 0% 7%;           /* Page background */
  --card: 0 0% 10%;                /* Card / panel surfaces */
  --popover: 0 0% 10%;             /* Dropdown menus, tooltips */
  --secondary: 0 0% 15%;           /* Secondary surfaces (tags, badges) */
  --muted: 0 0% 15%;               /* Muted / disabled surfaces */

  /* ── Foregrounds (text) ──────────────────────────── */
  --foreground: 0 0% 95%;          /* Primary text */
  --card-foreground: 0 0% 95%;     /* Text on card surfaces */
  --popover-foreground: 0 0% 95%;  /* Text in popovers */
  --secondary-foreground: 0 0% 95%;/* Text on secondary surfaces */
  --muted-foreground: 0 0% 64%;    /* Placeholder, hint, disabled text */

  /* ── Accent colors ──────────────────────────────── */
  --primary: 142 71% 45%;          /* Primary action (buttons, links, active) */
  --primary-foreground: 0 0% 100%; /* Text on primary-colored elements */
  --accent: 142 71% 45%;           /* Accent highlights (hover, focus) */
  --accent-foreground: 0 0% 100%;  /* Text on accent-colored elements */

  /* ── Destructive ────────────────────────────────── */
  --destructive: 0 84% 60%;        /* Delete, error, danger actions */
  --destructive-foreground: 0 0% 100%; /* Text on destructive elements */

  /* ── Borders & inputs ───────────────────────────── */
  --border: 0 0% 18%;              /* Borders, dividers, separators */
  --input: 0 0% 18%;               /* Input field borders */
  --ring: 142 71% 45%;             /* Focus ring outline color */

  /* ── Geometry ───────────────────────────────────── */
  --radius: 0.5rem;                /* Global border radius */
}
```

### Variable groups

#### Backgrounds

| Variable | Default | Used for |
|----------|---------|----------|
| `--background` | `0 0% 7%` | Main page / app background |
| `--card` | `0 0% 10%` | Cards, panels, elevated surfaces |
| `--popover` | `0 0% 10%` | Dropdowns, tooltips, context menus |
| `--secondary` | `0 0% 15%` | Tags, badges, secondary containers |
| `--muted` | `0 0% 15%` | Disabled / muted element backgrounds |

#### Foregrounds (text colors)

| Variable | Default | Used for |
|----------|---------|----------|
| `--foreground` | `0 0% 95%` | Primary body text |
| `--card-foreground` | `0 0% 95%` | Text inside card elements |
| `--popover-foreground` | `0 0% 95%` | Text inside popovers |
| `--secondary-foreground` | `0 0% 95%` | Text on secondary surfaces |
| `--muted-foreground` | `0 0% 64%` | Placeholder text, hints, timestamps |

#### Accent colors

| Variable | Default | Used for |
|----------|---------|----------|
| `--primary` | `142 71% 45%` | Buttons, active nav, links, progress bars |
| `--primary-foreground` | `0 0% 100%` | Text/icons on primary-colored buttons |
| `--accent` | `142 71% 45%` | Hover highlights, selected items |
| `--accent-foreground` | `0 0% 100%` | Text on accent-colored elements |

#### Destructive

| Variable | Default | Used for |
|----------|---------|----------|
| `--destructive` | `0 84% 60%` | Delete buttons, error states, warnings |
| `--destructive-foreground` | `0 0% 100%` | Text on destructive elements |

#### Borders & inputs

| Variable | Default | Used for |
|----------|---------|----------|
| `--border` | `0 0% 18%` | All borders, dividers, table lines |
| `--input` | `0 0% 18%` | Input field borders (text inputs, selects) |
| `--ring` | `142 71% 45%` | Focus ring outline (keyboard navigation) |

#### Geometry

| Variable | Default | Used for |
|----------|---------|----------|
| `--radius` | `0.5rem` | Global border radius for buttons, cards, inputs. Accepts any CSS length value. |

### Tips

- **`--primary`, `--accent`, and `--ring`** are often set to the same color. This gives the UI a consistent accent.
- **`--card` and `--popover`** are the same by default, but you can differentiate them if you want distinct elevated surfaces.
- For a **light theme**, swap the lightness values — make backgrounds high (90%+) and foregrounds low (10%–20%).
- You can add **additional custom CSS rules** beyond variable overrides. For example, you can change scrollbar styles, override specific component classes, or add custom `@font-face` declarations.

### Minimal color-only theme

If you only want to change the accent color, you can override just the relevant variables:

```css
/* Ocean Blue — minimal theme */
:root {
  --primary: 210 100% 50%;
  --accent: 210 100% 50%;
  --ring: 210 100% 50%;
}
```

Everything else inherits from the defaults.

---

## Allowed Files

Theme repositories can only contain files with the following extensions. All other file types are ignored during installation.

### Stylesheets

| Extension | Description |
|-----------|-------------|
| `.css` | CSS stylesheets |

### Images

| Extension | Description |
|-----------|-------------|
| `.png` | PNG images |
| `.jpg` | JPEG images |
| `.jpeg` | JPEG images (alternate extension) |
| `.webp` | WebP images |
| `.svg` | SVG vector images |

### Fonts

| Extension | Description |
|-----------|-------------|
| `.woff2` | WOFF2 fonts (recommended — smallest file size) |
| `.woff` | WOFF fonts |
| `.ttf` | TrueType fonts |
| `.otf` | OpenType fonts |

### Explicitly disallowed

The following file types are **never** processed, even if present in the repository:

- **JavaScript** — `.js`, `.mjs`, `.cjs`, `.ts`, `.mts`
- **HTML** — `.html`, `.htm`
- **Executables** — `.sh`, `.bat`, `.exe`, `.wasm`

Themes are CSS-only by design. No JavaScript execution is possible.

---

## Security Constraints

Themes are installed from external git repositories and served to all users, so SoundTime enforces strict security boundaries.

### Git transport

- **HTTPS only** — Theme repositories must use `https://` URLs. SSH (`git@`), `file://`, and `git://` protocols are rejected.
- Example: `https://github.com/user/theme.git` ✓
- Rejected: `git@github.com:user/theme.git` ✗

### No executable content

- No JavaScript, TypeScript, or HTML files are processed.
- Theme CSS is served with `Content-Type: text/css`. Browsers will not execute it as script.
- The server's Content-Security-Policy header restricts script execution to `'self'` (the SoundTime application itself). Injected `<script>` tags or JS event handlers in CSS have no effect.

### Path safety

- **No path traversal** — Paths containing `..` are rejected. A theme cannot reference files outside its own directory.
- **No symlinks** — Symbolic links within the theme repository are not followed.
- All file paths in `theme.toml` must be relative to the repository root.

### Size limits

- **Maximum theme size** — Configurable via the `THEME_MAX_SIZE_MB` environment variable. Default: **20 MB**.
- Themes exceeding this limit are rejected during installation.

### Content-Security-Policy

SoundTime sets the following CSP header, which applies to theme assets:

```
style-src 'self' 'unsafe-inline';
img-src 'self' data: blob: https:;
font-src 'self';
```

This means:
- **CSS** can use inline styles and load from the same origin.
- **Images** referenced in CSS via `url()` can load from `https://` origins, `data:` URIs, or the same origin.
- **Fonts** loaded via `@font-face` must be served from the same origin (i.e., bundled in the theme's assets directory). External font CDNs are blocked by the `font-src 'self'` directive.

### External URLs in CSS

You can reference external images in your CSS:

```css
.my-background {
  background-image: url("https://example.com/image.jpg"); /* Allowed */
}
```

For fonts, you must bundle them in the assets directory:

```css
@font-face {
  font-family: "CustomFont";
  /* Reference a font bundled in your theme's assets directory */
  src: url("/api/themes/assets/fonts/custom.woff2") format("woff2");
}
```

---

## Safe Mode

If a theme causes rendering issues (unreadable text, broken layout, blank page), users and admins can bypass it.

### Method 1: URL parameter

Append `?theme=default` to any SoundTime URL to load the page without the custom theme:

```
https://your-instance.com/?theme=default
https://your-instance.com/albums?theme=default
```

This applies to the current page load only.

### Method 2: localStorage flag (persistent)

Open your browser's developer console (<kbd>F12</kbd> → Console tab) and run:

```js
localStorage.soundtime_theme_safe = 1;
```

This disables the custom theme **persistently** across all pages and browser sessions until you remove the flag.

### Exiting safe mode

To re-enable the custom theme after setting the localStorage flag:

```js
localStorage.removeItem("soundtime_theme_safe");
```

Then refresh the page.

### For admins

If a theme is completely broken and prevents access to the admin panel:

1. Use safe mode (above) to bypass the theme.
2. Navigate to **Settings → Themes**.
3. Click **Disable** on the problematic theme.
4. Optionally **Uninstall** it.

---

## Testing Locally

### Option A: Browser DevTools (fastest)

The quickest way to prototype a theme is to override CSS variables directly in your browser:

1. Open SoundTime in your browser.
2. Open DevTools (<kbd>F12</kbd> → Elements tab).
3. Select the `<html>` element.
4. In the Styles panel, add a new rule:

```css
:root {
  --primary: 262 83% 58%;
  --accent: 262 83% 58%;
  --ring: 262 83% 58%;
  --background: 240 10% 4%;
}
```

5. Changes apply instantly. Iterate until you're happy, then copy the values into your `theme.css`.

### Option B: Full end-to-end test

To test the complete install flow:

1. **Clone SoundTime** and start the dev environment (see [development.md](development.md)).

2. **Create your theme repo** with `theme.toml` and `theme.css` as described in [Quick Start](#quick-start).

3. **Push to a git host** — any HTTPS-accessible git repository works. GitHub, GitLab, Gitea, Forgejo, or a self-hosted git server.

4. **Install via the admin panel** — paste the HTTPS clone URL and click Install.

5. **Enable the theme** and verify across different pages:
   - Home / library view
   - Album detail page
   - Player (both mini and full-screen)
   - Admin panel
   - Login / register pages
   - Mobile viewport (resize your browser or use DevTools responsive mode)

6. **Check contrast** — ensure text remains readable on all surfaces. Pay special attention to:
   - `--muted-foreground` against `--background` (timestamps, metadata)
   - `--primary-foreground` against `--primary` (button text)
   - `--destructive-foreground` against `--destructive` (delete confirmations)

### Option C: Local CSS override (no git needed)

For rapid iteration before pushing to git, you can inject your theme CSS using a browser extension like [Stylus](https://github.com/openstyles/stylus) or [User CSS](https://usercss.org/):

1. Install the extension.
2. Create a new style targeting your SoundTime instance's domain.
3. Paste your theme CSS.
4. Changes apply on page reload.

---

## Publishing

### Repository setup

1. **Public HTTPS repo** — push your theme to a public repository on GitHub, GitLab, Codeberg, or any git host accessible via HTTPS.

2. **Include a README** — describe the theme, include a screenshot or two, and list any fonts or images that require attribution.

3. **Tag releases** — use git tags matching your `theme.toml` version:

   ```sh
   git tag v1.0.0
   git push origin v1.0.0
   ```

4. **Follow semantic versioning**:
   - **Patch** (`1.0.1`) — bug fixes, minor color tweaks
   - **Minor** (`1.1.0`) — new asset additions, new variable overrides
   - **Major** (`2.0.0`) — complete redesigns, breaking changes from prior versions

### Recommended repository structure

```
soundtime-theme-neo-dark/
├── README.md             # Description + screenshots
├── LICENSE               # License file
├── theme.toml            # SoundTime theme manifest
├── theme.css             # Main stylesheet
└── assets/
    └── fonts/
        └── custom.woff2
```

### Naming convention

Theme repositories are conventionally named `soundtime-theme-<name>` (e.g. `soundtime-theme-neo-dark`). This makes them discoverable via search.

### Updating an installed theme

When you push changes to your repository, admins can update the installed theme via the admin panel or the API. The server pulls the latest code from the repository and applies it.

---

## API Reference

All theme endpoints are under `/api`. Public endpoints serve the active theme to all users. Admin endpoints require an authenticated admin user.

### Public endpoints

#### `GET /api/themes/active`

Returns metadata about the currently active theme. Returns `404` if no theme is active (default theme is in use).

**Response** `200 OK`
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "neo-dark",
  "version": "1.0.0",
  "description": "A purple-accented dark theme",
  "author": "Your Name"
}
```

#### `GET /api/themes/active.css`

Serves the active theme's CSS file. Returns `404` if no theme is active.

**Response** `200 OK`
```
Content-Type: text/css
```

This is the endpoint that the frontend loads to apply the theme.

#### `GET /api/themes/assets/{path}`

Serves a static asset from the active theme's assets directory. The `{path}` parameter is relative to the theme's declared `assets_dir`.

**Example:** If your theme has `assets/fonts/custom.woff2`, it is served at:
```
GET /api/themes/assets/fonts/custom.woff2
```

Use this path in your CSS `@font-face` and `url()` declarations.

### Admin endpoints

All admin endpoints require the `Authorization: Bearer <token>` header with an admin-role JWT.

#### `GET /api/admin/themes`

List all installed themes.

**Response** `200 OK`
```json
[
  {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "name": "neo-dark",
    "version": "1.0.0",
    "description": "A purple-accented dark theme",
    "author": "Your Name",
    "license": "MIT",
    "homepage": "https://github.com/yourname/soundtime-theme-neo-dark",
    "git_url": "https://github.com/yourname/soundtime-theme-neo-dark.git",
    "status": "enabled",
    "installed_at": "2026-01-15T10:30:00Z",
    "updated_at": "2026-01-15T10:30:00Z"
  }
]
```

The `status` field is one of: `enabled`, `disabled`.

#### `POST /api/admin/themes/install`

Install a theme from a git repository.

**Request body**
```json
{
  "git_url": "https://github.com/yourname/soundtime-theme-neo-dark.git"
}
```

**Response** `201 Created`
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "neo-dark",
  "version": "1.0.0",
  "status": "disabled"
}
```

The theme is installed in a **disabled** state. You must explicitly enable it.

**Errors:**
- `400` — Invalid git URL, missing `theme.toml`, invalid manifest, disallowed file types
- `409` — A theme with the same name is already installed
- `413` — Theme repository exceeds `THEME_MAX_SIZE_MB`

#### `POST /api/admin/themes/{id}/enable`

Enable a theme. Only one theme can be active at a time — enabling a theme automatically disables any previously active theme.

**Response** `200 OK`

#### `POST /api/admin/themes/{id}/disable`

Disable a theme. The instance reverts to the default SoundTime appearance.

**Response** `200 OK`

#### `POST /api/admin/themes/{id}/update`

Pull the latest changes from the theme's git repository and update the installed version.

**Response** `200 OK`
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "neo-dark",
  "version": "1.1.0",
  "status": "enabled"
}
```

#### `DELETE /api/admin/themes/{id}`

Uninstall a theme. If the theme is currently active, the instance reverts to the default appearance.

**Response** `204 No Content`

**Errors:**
- `404` — Theme not found

---

## Examples

### Warm dark theme

```css
/* Amber Glow — warm dark theme */
:root {
  --background: 20 15% 6%;
  --foreground: 35 20% 92%;
  --card: 20 15% 9%;
  --card-foreground: 35 20% 92%;
  --popover: 20 15% 9%;
  --popover-foreground: 35 20% 92%;
  --primary: 35 100% 50%;
  --primary-foreground: 20 15% 6%;
  --secondary: 20 15% 14%;
  --secondary-foreground: 35 20% 92%;
  --muted: 20 15% 14%;
  --muted-foreground: 20 10% 55%;
  --accent: 35 100% 50%;
  --accent-foreground: 20 15% 6%;
  --destructive: 0 84% 60%;
  --destructive-foreground: 0 0% 100%;
  --border: 20 15% 17%;
  --input: 20 15% 17%;
  --ring: 35 100% 50%;
  --radius: 0.5rem;
}
```

### Light theme

```css
/* Clean Light — light mode theme */
:root {
  --background: 0 0% 98%;
  --foreground: 0 0% 10%;
  --card: 0 0% 100%;
  --card-foreground: 0 0% 10%;
  --popover: 0 0% 100%;
  --popover-foreground: 0 0% 10%;
  --primary: 220 80% 50%;
  --primary-foreground: 0 0% 100%;
  --secondary: 0 0% 93%;
  --secondary-foreground: 0 0% 10%;
  --muted: 0 0% 93%;
  --muted-foreground: 0 0% 45%;
  --accent: 220 80% 50%;
  --accent-foreground: 0 0% 100%;
  --destructive: 0 84% 50%;
  --destructive-foreground: 0 0% 100%;
  --border: 0 0% 85%;
  --input: 0 0% 85%;
  --ring: 220 80% 50%;
  --radius: 0.5rem;
}
```

### Theme with custom font

```toml
# theme.toml
[theme]
name = "retro-mono"
version = "1.0.0"
description = "Monospace retro terminal theme"
author = "Your Name"
license = "MIT"

[assets]
css = "theme.css"
assets_dir = "assets"
```

```css
/* theme.css */
@font-face {
  font-family: "JetBrains Mono";
  src: url("/api/themes/assets/fonts/JetBrainsMono-Regular.woff2") format("woff2");
  font-weight: 400;
  font-style: normal;
  font-display: swap;
}

:root {
  --background: 120 5% 5%;
  --foreground: 120 40% 80%;
  --primary: 120 60% 50%;
  --primary-foreground: 120 5% 5%;
  --accent: 120 60% 50%;
  --accent-foreground: 120 5% 5%;
  --card: 120 5% 8%;
  --card-foreground: 120 40% 80%;
  --popover: 120 5% 8%;
  --popover-foreground: 120 40% 80%;
  --secondary: 120 5% 12%;
  --secondary-foreground: 120 40% 80%;
  --muted: 120 5% 12%;
  --muted-foreground: 120 20% 50%;
  --destructive: 0 84% 60%;
  --destructive-foreground: 0 0% 100%;
  --border: 120 10% 18%;
  --input: 120 10% 18%;
  --ring: 120 60% 50%;
  --radius: 0rem;
}

body {
  font-family: "JetBrains Mono", monospace;
}
```

### Scrollbar customization

SoundTime styles the webkit scrollbar by default. You can override it in your theme:

```css
::-webkit-scrollbar-track {
  background: hsl(var(--background));
}

::-webkit-scrollbar-thumb {
  background: hsl(var(--primary) / 0.3);
  border-radius: 4px;
}

::-webkit-scrollbar-thumb:hover {
  background: hsl(var(--primary) / 0.5);
}
```
