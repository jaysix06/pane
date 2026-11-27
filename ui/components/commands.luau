-- File commands shared by the keyboard, the toolbar and menus.

local state = require("../state")
local confirm = require("./confirm")

local M = {}

local function in_folder(): boolean
	local n = state.nav:peek()
	return n ~= nil and not n.overview and not n.this_pc
end

local function what(): string
	local sel = state.sel:peek()
	if sel.count == 1 then
		for id in sel.ids do
			return `"{state.display_name(id)}"`
		end
	end
	return `{sel.count} items`
end

-- Delete / Shift+Delete: to the Recycle Bin, or permanently after asking.
function M.delete(shift: boolean?)
	if not in_folder() or state.sel:peek().count == 0 then
		return
	end
	-- In the Recycle Bin, deleting is for good.
	if state.nav:peek() and (state.nav:peek() :: any).recycle then
		M.purge()
		return
	end
	local permanent = shift == true or state.setting("delete_permanently") == true
	if permanent then
		confirm.ask {
			title = "Delete permanently?",
			body = `{what()} will be deleted for good. This can't be undone.`,
			label = "Delete permanently",
			danger = true,
			action = "delete",
			arg = { permanent = true, confirmed = true },
		}
	elseif state.setting("confirm_delete") then
		confirm.ask {
			title = "Move to Recycle Bin?",
			body = `{what()} will go to the Recycle Bin. You can restore it from there.`,
			label = "Move to Recycle Bin",
			action = "delete",
			arg = { permanent = false },
		}
	else
		ui.invoke("delete", { permanent = false })
	end
end

function M.rename()
	if in_folder() and state.sel:peek().count > 0 then
		ui.invoke("start_rename")
	end
end

function M.new_folder()
	if in_folder() then
		ui.invoke("new_folder")
	end
end

-- The Recycle Bin: put the selection back where it was, or delete it for
-- good (after asking), or empty the whole bin (Windows asks).
function M.restore()
	if state.sel:peek().count > 0 then
		ui.invoke("restore")
	end
end

function M.purge()
	if state.sel:peek().count == 0 then
		return
	end
	confirm.ask {
		title = "Delete permanently?",
		body = `{what()} will be deleted for good. This can't be undone.`,
		label = "Delete permanently",
		danger = true,
		action = "purge",
	}
end

function M.empty_bin()
	ui.invoke("empty_recycle_bin")
end

-- An empty file from the New menu: "text" or "markdown".
function M.new_file(kind: string)
	if in_folder() then
		ui.invoke("new_file", kind)
	end
end

function M.copy()
	if in_folder() then
		ui.invoke("copy")
	end
end

function M.cut()
	if in_folder() then
		ui.invoke("cut")
	end
end

-- The selection's full paths as text, one per line.
function M.copy_path()
	if in_folder() and state.sel:peek().count > 0 then
		ui.invoke("copy_path")
	end
end

function M.paste()
	if in_folder() then
		ui.invoke("paste")
	end
end

M.in_folder = in_folder

-- Rust asks a row to start renaming (a new folder, the menu's Rename).
local last_request = 0
ui.effect(function()
	local r = state.rename_request:get()
	-- Only fresh requests: an old one (seen again after a reload) would
	-- put a row that may be gone into rename mode and hold the keyboard.
	if r and r.at ~= last_request and math.abs(ui.now() - r.at) < 3000 then
		last_request = r.at
		state.renaming:set(r.id)
	end
end)

return M
