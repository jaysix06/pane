-- File operations in the footer. One running copy, move or deletion shows
-- inline: icon, "Copying to D:\Backup", a 24-LED progress bar, percent and
-- time left. Two or more show "2 running". A click opens a popover above
-- the footer with today's operations as cards, each with pause and cancel.
-- A finished one stays in the footer a moment (a check, or what went
-- wrong), then only the popover lists it.
-- Rust: crates/pane-app/src/ops.rs.

local state = require("../state")
local theme = require("../theme")
local icons = require("./icons")
local meter = require("./meter")

local M = {}

-- 24 LEDs of 3 points, 1.5 apart.
local BAR_W = 24 * 4.5 - 1.5

local running = ui.computed(function(): { any }
	local out = {}
	for _, o in state.ops:get() do
		if not o.finished then
			table.insert(out, o)
		end
	end
	return out
end)

-- The operation the footer shows inline: the only running one, else the
-- one that just finished (when nothing else runs). nil for none.
local shown = ui.computed(function(): any
	local r = running:get()
	if #r == 1 then
		return r[1]
	elseif #r > 1 then
		return nil
	end
	local list = state.ops:get()
	for i = #list, 1, -1 do
		if list[i].recent then
			return list[i]
		end
	end
	return nil
end)

-- The popover's cards: the newest few, newest first.
local SHOWN_CARDS = 6
local newest_first = ui.computed(function(): { any }
	local list = state.ops:get()
	local out = table.create(SHOWN_CARDS)
	for i = #list, math.max(#list - SHOWN_CARDS + 1, 1), -1 do
		table.insert(out, list[i])
	end
	return out
end)

-- Anything for the footer to show.
function M.active(): boolean
	return #running:get() > 0 or shown:get() ~= nil
end

local function status_icon(o: () -> any, size: number): Node
	return ui.div {
		color = function()
			local x = o()
			return if x == nil
				then "$text.tertiary"
				elseif x.error then "$danger"
				elseif x.cancelled then "$text.tertiary"
				elseif x.finished then "$success"
				else "$accent"
		end,
		ui.switch(function()
			local x = o()
			if x == nil then
				return "running"
			end
			return if x.error then "error" elseif x.cancelled then "cancelled" elseif x.finished then "done" else "running"
		end, {
			error = icons.icon("warning", size),
			cancelled = icons.icon("close", size),
			done = icons.icon("check", size),
			running = ui.switch(function()
				local x = o()
				return if x then x.kind else "copy"
			end, {
				copy = icons.icon("copy", size),
				move = icons.icon("cut", size),
				delete = icons.icon("trash", size),
				rename = icons.icon("rename", size),
				new = icons.icon("folder_plus", size),
			}),
		}),
	}
end

local function bar(progress: () -> number, scanning: () -> boolean, props: { [string]: any }): Node
	local node: { [string]: any } = {
		data = function()
			return { bytes = { apps = progress() }, total = 1 }
		end,
		scanning = scanning,
		glow = function()
			return (state.setting("glow") or 70) / 100 * 0.7
		end,
		motion = theme.motion(state.setting),
		dot = 3,
		gap = 1.5,
		label = "Progress",
	}
	for k, v in props do
		node[k] = v
	end
	return meter.meter(node)
end

-- "42%" while running; how it ended once done.
local function outcome(x: any): string
	if x.error then
		return "Failed"
	elseif x.cancelled then
		return "Cancelled"
	elseif x.finished then
		return "Done"
	elseif x.paused then
		return "Paused"
	end
	return `{math.floor((x.progress or 0) * 100)}%`
end

local function small_button(label: string, icon: () -> string, on_click: () -> ()): Node
	return ui.button {
		label = label,
		tooltip = label,
		on_click = on_click,
		w = 24,
		h = 24,
		items = "center",
		justify = "center",
		rounded = "$radius.sm",
		bg = "#00000000",
		color = "$text.secondary",
		hover = { bg = "$hover", color = "$text.primary" },
		active = { bg = "$pressed" },
		ui.switch(icon, {
			pause = icons.icon("pause", 14),
			play = icons.icon("play", 14),
			close = icons.icon("close", 14),
		}),
	}
end

-- One of today's operations, in the popover.
local function card(op: any, _index: number, row: State<any>): Node
	local function o(): any
		return row:get()
	end
	local function live(): boolean
		return not o().finished
	end
	return ui.column {
		w_full = true,
		p = 10,
		gap = 7,
		rounded = "$radius.md",
		bg = "$surface.base",
		border = 1,
		border_color = function()
			return if o().error then "$danger" else "$border.subtle"
		end,
		ui.row {
			w_full = true,
			gap = 9,
			items = "center",
			status_icon(o, 15),
			ui.column {
				grow = 1,
				min_w = 0,
				gap = 1,
				ui.text {
					function()
						return o().label
					end,
					literal = true,
					truncate = true,
					text_size = 12.5,
				},
				ui.text {
					function()
						local x = o()
						local bits = { x.at or "" }
						table.insert(bits, outcome(x))
						if x.eta and x.eta ~= "" then
							table.insert(bits, x.eta)
						end
						return table.concat(bits, " · ")
					end,
					font = "$font.mono",
					text_size = 10.5,
					color = "$text.tertiary",
					truncate = true,
				},
			},
			ui.show(function()
				return live() and o().can_pause == true
			end, small_button("Pause", function()
				return if o().paused then "play" else "pause"
			end, function()
				local x = o()
				ui.invoke("op_pause", { id = x.id, paused = not x.paused })
			end)),
			-- Not labelled "Cancel": zgx takes a button named that in a popover
			-- as the popover's own dismiss button, and its on_click never runs.
			ui.show(live, small_button("Stop", function()
				return "close"
			end, function()
				ui.invoke("op_cancel", o().id)
			end)),
		},
		ui.show(live, bar(function()
			return o().progress or 0
		end, function()
			local x = o()
			return not x.finished and not x.paused
		end, { w_full = true, h = 8 })),
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

local function popover_body(): { Node }
	return {
		ui.row {
			w_full = true,
			items = "center",
			pb = 2,
			ui.text {
				"TODAY",
				font = "$font.mono",
				text_size = 10.5,
				letter_spacing = 1.2,
				color = "$text.tertiary",
				grow = 1,
			},
			ui.show(
				function()
					for _, o in state.ops:get() do
						if o.finished then
							return true
						end
					end
					return false
				end,
				ui.button {
					label = "Clear finished",
					on_click = function()
						ui.invoke("ops_clear")
					end,
					px = 6,
					py = 2,
					rounded = "$radius.sm",
					bg = "#00000000",
					color = "$text.secondary",
					hover = { bg = "$hover", color = "$text.primary" },
					ui.text { "Clear finished", text_size = 11.5 },
				}
			),
		},
		-- A plain column: a scroll's bar would sit over the cards' Cancel buttons.
		ui.column {
			w_full = true,
			gap = 6,
			ui.each(newest_first, card),
		},
	}
end

-- The footer's operations cell; `name` keeps each footer's popover apart.
function M.cell(name: string): Node
	local open = ui.state(`ops_popover_{name}`, false)
	local function x(): any
		return shown:get()
	end
	local inline = ui.row {
		gap = 8,
		items = "center",
		min_w = 0,
		status_icon(x, 13),
		ui.text {
			function()
				local o = x()
				return if o then (o.error or o.short or o.label) else ""
			end,
			literal = true,
			truncate = true,
			min_w = 0,
			grow = 1,
			shrink = 1,
			text_size = 11.5,
			color = "$text.secondary",
		},
		ui.show(function()
			local o = x()
			return o ~= nil and not o.finished
		end, bar(function()
			local o = x()
			return if o then o.progress or 0 else 0
		end, function()
			local o = x()
			return o ~= nil and not o.finished and not o.paused
		end, { w = BAR_W, h = 4, shrink = 0 })),
		ui.text {
			function()
				local o = x()
				return if o then outcome(o) else ""
			end,
			font = "$font.mono",
			text_size = 11,
			color = "$text.tertiary",
			shrink = 0,
		},
		ui.text {
			function()
				local o = x()
				return if o and o.eta then o.eta else ""
			end,
			font = "$font.mono",
			text_size = 11,
			color = "$text.tertiary",
			truncate = true,
			min_w = 0,
			shrink = 2,
		},
	}
	local many = ui.row {
		gap = 8,
		items = "center",
		ui.div { color = "$accent", icons.icon("dots", 13) },
		ui.text {
			function()
				return `{#running:get()} running`
			end,
			text_size = 11.5,
			color = "$text.secondary",
		},
	}
	return ui.popover {
		open = open,
		label = "File operations",
		placement = { side = "top", align = "center", offset = 6 },
		w = 340,
		p = 10,
		gap = 8,
		rounded = "$radius.lg",
		anchor = ui.button {
			label = "File operations",
			tooltip = "Show today's file operations",
			on_click = ui.toggle(open),
			h = 22,
			px = 8,
			min_w = 0,
			shrink = 1,
			items = "center",
			rounded = "$radius.sm",
			bg = "#00000000",
			hover = { bg = "$hover" },
			on_mount = function(el)
				if theme.motion(state.setting)() then
					anime.animate(el, { opacity = { 0, 1 }, duration = 200 })
				end
			end,
			ui.switch(function()
				return if #running:get() > 1 then "many" else "one"
			end, { one = inline, many = many }),
		},
		table.unpack(popover_body()),
	}
end

return M
