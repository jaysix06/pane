-- The operations panel: running copies, moves and deletions as small cards
-- at the bottom right, each with an LED progress strip. Finished ones show
-- a check (or what went wrong) for a moment, then leave.
-- Rust: crates/pane-app/src/ops.rs.

local state = require("../state")
local theme = require("../theme")
local icons = require("./icons")
local meter = require("./meter")

local KIND_ICON = {
	copy = "copy",
	move = "cut",
	delete = "trash",
	rename = "rename",
	new = "folder_plus",
}

local function card(op: any, _index: number, row: State<any>): Node
	local function o(): any
		return row:get()
	end
	return ui.column {
		w = 300,
		p = 12,
		gap = 8,
		rounded = "$radius.lg",
		bg = "$surface.overlay",
		border = 1,
		border_color = function()
			return if o().error then "$danger" else "$border.subtle"
		end,
		shadow = "$shadow.md",
		inset_shadow = { x = 0, y = 1, color = "$app.highlight" },
		on_mount = function(el)
			if theme.motion(state.setting)() then
				anime.animate(el, { opacity = { 0, 1 }, translateY = { 10, 0 }, duration = 260, ease = "out(3)" })
			end
		end,
		ui.row {
			w_full = true,
			gap = 10,
			items = "center",
			ui.div {
				color = function()
					local x = o()
					return if x.error then "$danger" elseif x.finished then "$success" else "$accent"
				end,
				ui.switch(function()
					local x = o()
					return if x.error then "error" elseif x.finished then "done" else "running"
				end, {
					error = icons.icon("warning", 16),
					done = icons.icon("check", 16),
					running = icons.icon(KIND_ICON[op.kind] or "dots", 16),
				}),
			},
			ui.text {
				function()
					return o().label
				end,
				literal = true,
				truncate = true,
				grow = 1,
				min_w = 0,
			},
			ui.text {
				function()
					local x = o()
					return if x.finished then "" else `{math.floor((x.progress or 0) * 100)}%`
				end,
				font = "$font.mono",
				text_size = 11,
				color = "$text.tertiary",
			},
		},
		ui.show(
			function()
				return o().error == nil
			end,
			meter.meter {
				data = function()
					return { bytes = { apps = o().progress or 0 }, total = 1 }
				end,
				scanning = function()
					return not o().finished
				end,
				glow = function()
					return (state.setting("glow") or 70) / 100 * 0.7
				end,
				motion = theme.motion(state.setting),
				dot = 3,
				gap = 1.5,
				w_full = true,
				h = 8,
				label = "Progress",
			}
		),
		ui.show(
			function()
				return o().error ~= nil
			end,
			ui.text {
				function()
					return o().error or ""
				end,
				literal = true,
				wrap = true,
				lines = 3,
				text_size = 12,
				color = "$text.secondary",
			}
		),
	}
end

return function(): Node
	return ui.column {
		absolute = true,
		right = 16,
		bottom = 40,
		gap = 8,
		items = "end",
		pointer_events = "none",
		ui.each(state.ops, card),
	}
end
