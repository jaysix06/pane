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
	sort: string,
	descending: boolean,
	view: string,
	loading: boolean,
	error: string?,
}
export type Tab = { id: number, title: string, path: string, loading: boolean }
export type Place = { label: string, path: string, icon: string, id: string?, used: number?, detail: string?, pinned: boolean? }
export type Places = { pinned: { Place }, known: { Place }, drives: { Place } }
export type Sel = { ids: { [string]: boolean }, count: number, focus: number }

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

-- UI-only state.
M.editing_address = ui.state("editing_address", false)
M.address_text = ui.state("address_text", "")
M.grid_cols = ui.state("grid_cols", 6)
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

-- Host images for file icons: "host:s/<key>" (32 px) or "host:l/<key>" (96 px).
function M.icon_src(key: string, large: boolean?): string
	return (if large then "host:l/" else "host:s/") .. key
end

return M
