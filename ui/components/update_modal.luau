-- The update dialog, centred: download progress, then "Update now" /
-- "Update later". Rust: crates/pane-app/src/updates.rs (shared "update").

local state = require("../state")
local icons = require("./icons")

local M = {}

M.open = ui.state("update_modal_open", false)

local function u(): any
	return state.update:get() or {}
end

local function phase(): string
	return u().phase or ""
end

-- "Get the update": start downloading and show the dialog.
function M.start()
	local cur = state.update:peek()
	if cur and not cur.can_download then
		-- Nothing to download and check: open the release page instead.
		ui.invoke("update_open")
		return
	end
	if cur and cur.phase == "error" then
		ui.invoke("update_retry")
	end
	ui.invoke("update_download")
	M.open:set(true)
end

local function secondary(label: string, on_click: () -> ()): Node
	return ui.button {
		label = label,
		on_click = on_click,
		px = 14,
		h = 34,
		rounded = "$radius.md",
		bg = "$surface.raised",
		border = 1,
		border_color = "$border.subtle",
		hover = { border_color = "$border.strong" },
		ui.text { label },
	}
end

local function primary(label: string, icon: string, on_click: () -> ()): Node
	return ui.button {
		label = label,
		on_click = on_click,
		px = 16,
		h = 34,
		gap = 8,
		items = "center",
		rounded = "$radius.md",
		bg = "$accent",
		color = "$accent.text",
		shadow = { x = 0, y = 0, blur = 14, spread = 0, color = "$app.glow" },
		hover = { bg = "$accent.hover" },
		on_mount = function(el)
			el:focus()
		end,
		icons.icon(icon, 14, "$accent.text"),
		ui.text { label, weight = 600 },
	}
end

-- The progress bar: a track with an accent fill that glows at its edge.
local function progress_bar(): Node
	return ui.div {
		w_full = true,
		h = 8,
		rounded = "$radius.full",
		bg = "$surface.sunken",
		border = 1,
		border_color = "$border.subtle",
		overflow = "hidden",
		ui.div {
			h_full = true,
			w = function()
				local p = math.clamp(u().progress or 0, 0, 1)
				return `{math.max(2, math.floor(p * 1000) / 10)}%`
			end,
			rounded = "$radius.full",
			bg = "$accent",
			shadow = { x = 0, y = 0, blur = 10, spread = 0, color = "$app.glow" },
		},
	}
end

local function header(icon: string, title: () -> string, sub: () -> string): Node
	return ui.row {
		w_full = true,
		gap = 12,
		items = "center",
		ui.div {
			w = 40,
			h = 40,
			rounded = "$radius.lg",
			bg = "$selection.item",
			items = "center",
			justify = "center",
			shrink = 0,
			icons.icon(icon, 20, "$accent"),
		},
		ui.column {
			grow = 1,
			min_w = 0,
			gap = 3,
			ui.text { title, weight = 600, text_size = 16 },
			ui.text { sub, color = "$text.secondary", text_size = 12.5, wrap = true },
		},
	}
end

local function downloading(): Node
	return ui.column {
		w_full = true,
		gap = 16,
		header("download", function()
			return `Downloading Pane {u().version or ""}`
		end, function()
			return "You can keep using Pane while it downloads."
		end),
		ui.column {
			w_full = true,
			gap = 8,
			progress_bar(),
			ui.row {
				w_full = true,
				ui.text {
					function()
						return `{u().done_text or ""} of {u().total_text or ""}`
					end,
					font = "$font.mono",
					text_size = 11.5,
					color = "$text.tertiary",
					grow = 1,
				},
				ui.text {
					function()
						return `{math.floor((u().progress or 0) * 100)}%`
					end,
					font = "$font.mono",
					text_size = 11.5,
					color = "$accent",
				},
			},
		},
		ui.row {
			w_full = true,
			justify = "end",
			secondary("Hide", function()
				M.open:set(false)
			end),
		},
	}
end

local function ready(): Node
	return ui.column {
		w_full = true,
		gap = 16,
		header("check", function()
			return `Pane {u().version or ""} is ready`
		end, function()
			local notes = u().notes or ""
			return if notes ~= "" then notes else "Downloaded and checked. Pane will close, update, and open again."
		end),
		progress_bar(),
		ui.row {
			w_full = true,
			gap = 8,
			justify = "end",
			secondary("Update later", function()
				M.open:set(false)
			end),
			primary("Update now", "refresh", function()
				M.open:set(false)
				ui.invoke("update_install")
			end),
		},
	}
end

local function failed(): Node
	return ui.column {
		w_full = true,
		gap = 16,
		header("warning", function()
			return "The update couldn't be downloaded"
		end, function()
			local e = u().error or ""
			return string.upper(string.sub(e, 1, 1)) .. string.sub(e, 2) .. "."
		end),
		ui.row {
			w_full = true,
			gap = 8,
			justify = "end",
			secondary("Close", function()
				M.open:set(false)
			end),
			primary("Try again", "refresh", function()
				ui.invoke("update_retry")
				ui.invoke("update_download")
			end),
		},
	}
end

function M.dialog(): Node
	return ui.modal {
		open = M.open,
		label = "Update Pane",
		w = 440,
		p = 0,
		bg = "#00000000",
		build = function()
			return ui.column {
				w = 440,
				p = 24,
				bg = "$surface.overlay",
				rounded = "$radius.xl",
				border = 1,
				border_color = "$border.subtle",
				shadow = "$shadow.lg",
				inset_shadow = { x = 0, y = 1, color = "$app.highlight" },
				ui.switch(phase, {
					downloading = downloading(),
					ready = ready(),
					error = failed(),
				}, downloading()),
			}
		end,
	}
end

return M
