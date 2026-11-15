# Mica PoC — GitHub scaffolding status

Date: 2026-11-15 (JST). Repo: `taka-kawah/mica-poc` (public, was empty, description "museでpoc", default branch `main`).
Scaffolding agent: GitHub scaffolding subagent. Code push is the coordinator's job, not this file's.

## 1. Status / accounts

- `/opt/hatch/bin/github status`: `connected`, `auth_status: connected`, `authenticated: true`, provider `github`, server `github-mcp-server` (protocol `2025-03-26`).
- `accounts`: one linked account, default: `taka-kawah` (account_id `95bac8377c7d4c71abcb1ddc9f57c708`, `needs_reauth: false`).
- `list-tools`: 46 live tools. No tool for GitHub Projects and no tool for Wiki exists in the list (see §4–5).

## 2. Write-access test — ✅ SUCCEEDED

- Method: `create_or_update_file` on new path `docs/progress-scaffold-test.md`, branch `main`, commit message `chore: scaffolding write-access test file`.
- Result: success. Commit SHA `0345b214f4e486ac6d94f6bb617f01a647c369ca`
  (https://github.com/taka-kawah/mica-poc/commit/0345b214f4e486ac6d94f6bb617f01a647c369ca),
  file blob SHA `c1baae50a20fbf73725664e49b4d2b201893a193`, file URL https://github.com/taka-kawah/mica-poc/blob/main/docs/progress-scaffold-test.md
- Note: the repo was empty before this commit, so this first commit created the `main` branch. Read check beforehand: `list_issues` returned 0 issues; `get_file_contents /` returned `409 Git Repository is empty` (expected for an empty repo).
- Each `call-tool` write required fresh approval and was approved (`userConfirmationStatus: approved`).

## 3. Issues created — ✅ all 7, verified via `list_issues` (totalCount 7, all OPEN)

No labels were applied: the `issue_write` schema supports a `labels` array, but the repo had no custom labels and inventing label names risked failures, so issues were created plain. Labels can be added later in the web UI.

| # | Title | URL |
|---|-------|-----|
| 1 | Mica PoC — plan, scope and evaluation | https://github.com/taka-kawah/mica-poc/issues/1 |
| 2 | PoC: Tauri + CodeMirror 6 | https://github.com/taka-kawah/mica-poc/issues/2 |
| 3 | PoC: GPUI | https://github.com/taka-kawah/mica-poc/issues/3 |
| 4 | PoC: egui / eframe | https://github.com/taka-kawah/mica-poc/issues/4 |
| 5 | Shared core: live-preview decoration engine, file model, WSL stdio RPC stub | https://github.com/taka-kawah/mica-poc/issues/5 |
| 6 | CI: GitHub Actions for Linux (+ Windows where feasible) | https://github.com/taka-kawah/mica-poc/issues/6 |
| 7 | Evaluation and decision: comparison table and GUI-tech recommendation | https://github.com/taka-kawah/mica-poc/issues/7 |

Each issue body states goal, acceptance criteria, known limits (Linux VM, no Windows visual check), and links back to #1. #1 links the Notion spec pages (https://app.notion.com/p/Mica-419a547957908176a738d87bb871c387 and https://app.notion.com/p/419a547957908167b406e69abcd78011). #3 notes: no GPL Zed editor code reuse. #7 depends on #2–#4 and #6.

## 4. GitHub Project — ❌ NOT created (no tool exists)

- `list-tools` (46 tools) contains no project tool — no `create_project` or similar. This was verified by name search over the live tool list; nothing was guessed or attempted via API.
- Action for the user: create the project in the GitHub web UI. Repo project page: https://github.com/taka-kawah/mica-poc/projects — or, for a user-level project: https://github.com/orgs/taka-kawah/projects (note: `taka-kawah` is a personal account, not an org, so the repo-level URL is the one to use; the orgs URL is listed here only because the task asked for both — it could NOT be verified as valid for a personal account).
- Suggested setup: a board with the 7 issues above; columns e.g. Todo / In progress / Done.

## 5. Wiki — ❌ NOT created (no tool exists)

- `list-tools` contains no wiki tool. GitHub wikis are also a separate git repo, so nothing was attempted by guessing.
- Action for the user: enable/create the Wiki in the repo web UI if wanted (repo Settings → Features → Wikis, then the Wiki tab). Until then, progress lives in README/docs in the repo itself (e.g. this status file and `docs/progress-scaffold-test.md`, which the coordinator can push/keep or remove).

## 6. What the user still needs to do

1. Nothing needed for GitHub App installation — write access already works (see §2).
2. (Optional) Create a GitHub Project at https://github.com/taka-kawah/mica-poc/projects and add issues #1–#7.
3. (Optional) Enable the repo Wiki if wanted; otherwise track progress in README/docs.
4. (Optional) Add labels to the 7 issues in the web UI (e.g. `poc`, `tracking`, `ci`, `evaluation`) — none were applied by the scaffolding agent.
5. Coordinator follow-up: push PoC code and consider keeping or deleting `docs/progress-scaffold-test.md` (write-access test file, commit `0345b214`).
