-- What Rust publishes (crates/pane-app/src/app.rs) and the UI's own state.
-- Rust owns the model; the UI reads it and calls actions (ui.invoke).
-- Rule: never ui.invoke from inside ui.effect / ui.computed (Rust may be
-- publishing at that moment); invoke from event handlers only.

export type Row = {
	id: string,
	name: string,
	dir: boolean,
	kind: string,
	date: string,
	size: string,
	icon: string,
	-- The thumbnail's host image name (pictures and videos), "" for none.
	thumb: string,
}
export type Crumb = { label: string, path: string }
export type Nav = {
	can_back: boolean,
	can_forward: boolean,
	can_up: boolean,
	crumbs: { Crumb },
	address: string,
	path: string,
	this_pc: boolean,
	overview: boolean,
	sort: string,
	descending: boolean,
	view: string,
	loading: boolean,
	error: string?,
	-- Search results: the query (path is the folder searched).
	search: string?,
}
export type Tab = { id: number, title: string, path: string, loading: boolean }
export type Place = { label: string, path: string, icon: string, id: string?, used: number?, detail: string?, pinned: boolean? }
export type Places = { pinned: { Place }, known: { Place }, drives: { Place } }
-- The selection: ids, how many, the focused row, files and folders among
-- them, and the files' total size ("13 KB", "" when not known).
export type Sel = {
	ids: { [string]: boolean },
	count: number,
	focus: number,
	files: number?,
	folders: number?,
	size: string?,
}

local M = {}

M.rows = ui.shared("rows", {} :: { Row })
M.nav = ui.shared("nav", nil :: Nav?)
M.tabs = ui.shared("tabs", {} :: { Tab })
M.active = ui.shared("active", 1)
M.sel = ui.shared("sel", { ids = {}, count = 0, focus = 0 } :: Sel)
M.status = ui.shared("status", "")
M.places = ui.shared("places", nil :: Places?)
M.settings = ui.shared("settings", {} :: { [string]: any })
M.toast = ui.shared("toast", nil :: { message: string, at: number }?)
-- The Storage Overview (crates/pane-app/src/storage.rs).
M.storage = ui.shared("storage", nil :: any)
-- The preview pane's content (crates/pane-app/src/preview.rs): nil while it
-- is closed; { state = "empty" | "many" | "item", ... }.
M.preview = ui.shared("preview", nil :: any)

-- A newer release on GitHub: { version, current, name, url, notes }, or nil.
M.update = ui.shared("update", nil :: any)
-- The last update check: "checking", "current", "available" or "error".
M.update_check = ui.shared("update_check", nil :: string?)
-- The About dialog (crates/pane-app/src/about.rs): version, windows,
-- default_fm (Pane opens folders), default_error.
M.about = ui.shared("about", nil :: any)
-- File operations in progress (crates/pane-app/src/ops.rs).
M.ops = ui.shared("ops", {} :: { any })
-- Files on the clipboard: { count, cut } ("Paste 2 items").
M.clipboard = ui.shared("clipboard", { count = 0, cut = false } :: { count: number, cut: boolean })
-- Cut items: { folder, ids }, dimmed while that folder shows.
M.cut = ui.shared("cut", {} :: any)
-- Rust asks a row to start renaming: { id, at }.
M.rename_request = ui.shared("rename", nil :: { id: string, at: number }?)

-- UI-only state.
-- The row being renamed in place (its id), or nil.
M.renaming = ui.state("renaming", nil :: string?)
-- When a rename last ended (ui.now()), so its Enter isn't taken as "open".
M.rename_ended_at = 0
M.editing_address = ui.state("editing_address", false)
M.address_text = ui.state("address_text", "")
-- The search box's text, and its element (Ctrl+F focuses it).
M.search_text = ui.state("search_text", "")
M.search_ref = ui.ref()
M.grid_cols = ui.state("grid_cols", 6)
-- The folder view's width: details drops columns as it narrows.
M.view_w = ui.state("view_w", 1000)
M.list_ref = ui.ref()

-- Small readers.
function M.view(): string
	local n = M.nav:get()
	return if n then n.view else "details"
end

function M.setting(name: string): any
	return (M.settings:get() :: any)[name]
end

function M.is_selected(id: string): boolean
	local ids = M.sel:get().ids
	return ids ~= nil and ids[id] == true
end

function M.is_cut(id: string): boolean
	local c = M.cut:get()
	local n = M.nav:get()
	return c ~= nil and c.ids ~= nil and c.ids[id] == true and n ~= nil and c.folder == n.path
end

-- The full path of an item in the current folder.
function M.path_of(id: string): string
	local n = M.nav:peek()
	local dir = if n then n.path else ""
	if string.sub(dir, -1) == "\\" then
		return dir .. id
	end
	return dir .. "\\" .. id
end

-- Host images for file icons: "host:s/<key>" (32 px) or "host:l/<key>" (96 px).
function M.icon_src(key: string, large: boolean?): string
	return (if large then "host:l/" else "host:s/") .. key
end

return M
