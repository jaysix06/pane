-- The Storage Overview: Pane's home. A hero card with a counting readout,
-- the live scan path, the LED dot-matrix meter and its legend (hover to
-- isolate a category, click to see its biggest places), drive chips, and
-- the "things you can clear" cards. Rust: crates/pane-app/src/storage.rs.

local state = require("../state")
local theme = require("../theme")
local common = require("./common")
local icons = require("./icons")
local meter = require("./meter")
local confirm = require("./confirm")

local motion = theme.motion(state.setting)
local hovered = ui.state("overview_hover", 0)

-- The category to isolate on the bar: the hovered one, unless it holds
-- nothing on this drive (then there's nothing to show, and dimming every
-- band would leave the bar dark).
local function highlighted(): number
	local h = hovered:get()
	if h == 0 then
		return 0
	end
	local c = theme.categories[h]
	local bytes = ((state.storage:get() or {}).bytes or {})[c and c.id or ""] or 0
	return if bytes > 0 then h else 0
end

local function storage(): any
	return state.storage:get() or {}
end

local function field(name: string): () -> any
	return function()
		return (state.storage:get() or {})[name]
	end
end

local function glow(): number
	return (state.setting("glow") or 70) / 100
end

-- A path shortened in the middle to about `max` characters, keeping the
-- drive and the last parts: long paths with no spaces can't be cut at a
-- word, so the text's own truncation would leave just "C:...".
local function short_path(path: string, max: number): string
	if #path <= max then
		return path
	end
	local parts = string.split(path, "\\")
	local head = parts[1]
	local tail = parts[#parts]
	local i = #parts - 1
	while i > 1 and #head + #tail + #parts[i] + 6 <= max do
		tail = parts[i] .. "\\" .. tail
		i -= 1
	end
	local out = head .. "\\…\\" .. tail
	if #out > max then
		out = string.sub(out, 1, max - 3) .. "..."
	end
	return out
end

local function category_meta(id: string)
	return theme.categories[theme.category_index[id] or 11]
end

-- Hairline grid with a warm vignette, behind everything.
local backdrop = ui.shader([[
fn fill(input: FillInput) -> vec4<f32> {
	let pitch = 28.0;
	let g = abs(fract(input.p / pitch) - vec2<f32>(0.5)) * pitch;
	let line = 1.0 - smoothstep(0.0, 1.0 / max(input.scale, 1.0), min(g.x, g.y) - (pitch * 0.5 - 0.5));
	let uv = input.p / max(input.size, vec2<f32>(1.0));
	let centre = length((uv - vec2<f32>(0.5, 0.18)) * vec2<f32>(1.0, 1.6));
	let warm = exp(-centre * centre * 3.0);
	let tint = input.params[0];
	return vec4<f32>(tint.rgb, line * tint.a * 0.6 + warm * tint.a * 1.4);
}]])

-- Drive chips ---------------------------------------------------------------

local function drive_chip(d: any): Node
	return ui.button {
		label = d.label,
		on_click = ui.action("storage_select", d.letter),
		pl = 12,
		pr = 14,
		py = 8,
		gap = 12,
		items = "center",
		rounded = "$radius.lg",
		bg = if d.selected then "$surface.raised" else "#00000000",
		border = 1,
		border_color = if d.selected then "$border.strong" else "$border.subtle",
		hover = { bg = "$hover" },
		ui.column {
			gap = 6,
			ui.row {
				gap = 8,
				items = "center",
				ui.div { color = if d.selected then "$accent" else "$text.secondary", icons.icon("drive", 15) },
				ui.text { d.label, literal = true, weight = 500 },
			},
			meter.meter {
				data = function()
					return {
						bytes = if d.categories then d.categories else { apps = d.used },
						total = d.total,
					}
				end,
				glow = function()
					return glow() * 0.6
				end,
				motion = motion,
				dot = 3,
				gap = 1.5,
				w = 150,
				h = 8,
				label = `{d.label}: {d.used_text} of {d.total_text}`,
			},
			ui.text { `{d.used_text} of {d.total_text}`, literal = true, font = "$font.mono", text_size = 11, color = "$text.tertiary" },
		},
	}
end

local function drives_row(): Node
	local drives = ui.computed(function()
		return storage().drives or {}
	end)
	return ui.show(
		function()
			return #drives:get() > 1
		end,
		ui.row {
			gap = 8,
			w_full = true,
			flex_wrap = true,
			ui.each(drives, function(d: any)
				return drive_chip(d)
			end),
		}
	)
end

-- Hero ------------------------------------------------------------------------

local function readout(): Node
	return ui.row {
		items = "end",
		gap = 10,
		ui.text {
			field("used_text"),
			font = "$font.mono",
			text_size = 44,
			weight = 500,
			letter_spacing = -1,
		},
		ui.text {
			function()
				local s = storage()
				return if s.total_text then `of {s.total_text} used` else ""
			end,
			color = "$text.secondary",
			text_size = 15,
			pb = 5,
		},
	}
end

local function mode_badge(): Node
	return ui.row {
		gap = 6,
		items = "center",
		px = 8,
		h = 22,
		rounded = "$radius.full",
		border = 1,
		border_color = "$border.subtle",
		ui.div {
			w = 6,
			h = 6,
			rounded = "$radius.full",
			bg = function()
				return if storage().mode == "instant" then "$accent" else "$text.tertiary"
			end,
		},
		ui.text {
			function()
				return if storage().mode == "instant" then "INSTANT" else "STANDARD"
			end,
			font = "$font.mono",
			text_size = 10,
			letter_spacing = 1,
			color = "$text.tertiary",
		},
	}
end

local function rescan_button(): Node
	local is_scanning = ui.computed(function()
		return storage().phase == "scanning"
	end)
	local scanning = function()
		return is_scanning:get()
	end
	return ui.button {
		label = "Rescan",
		on_click = ui.action("storage_scan"),
		disabled = scanning,
		px = 12,
		h = 32,
		gap = 8,
		items = "center",
		rounded = "$radius.md",
		bg = "$surface.sunken",
		border = 1,
		border_color = "$border.subtle",
		color = "$text.secondary",
		hover = { color = "$text.primary", border_color = "$border.strong" },
		ui.div {
			ref = nil,
			on_mount = function(el)
				ui.effect(function()
					if scanning() and motion() then
						anime.animate(el, { rotate = { 0, 360 }, duration = 1100, loop = true, ease = "linear" })
					else
						anime.utils.remove(el)
						anime.animate(el, { rotate = 0, duration = 0 })
					end
				end)
			end,
			icons.icon("refresh", 14),
		},
		ui.text {
			function()
				return if scanning() then `Scanning… {math.floor((storage().permille or 0) / 10)}%` else "Rescan"
			end,
			font = "$font.mono",
			text_size = 12,
		},
	}
end

local function legend_item(c: any, index: number): Node
	local function data(): any
		for _, x in storage().categories or {} do
			if x.id == c.id then
				return x
			end
		end
		return { bytes = 0, text = "0 B", share = 0 }
	end
	local function dim(): boolean
		local h = highlighted()
		return h ~= 0 and h ~= index
	end
	return ui.row {
		role = "button",
		label = function()
			return `{c.label}: {data().text}`
		end,
		gap = 10,
		items = "center",
		px = 10,
		py = 8,
		rounded = "$radius.md",
		hover = { bg = "$hover" },
		opacity = ui.tween(ui.computed(dim), { from = 1, to = 0.38, duration = 160 }),
		on_hover = function(inside: boolean)
			if inside then
				hovered:set(index)
			elseif hovered:peek() == index then
				-- Only clear our own: moving straight to the next item can
				-- report its enter before this leave.
				hovered:set(0)
			end
		end,
		on_click = function()
			ui.invoke("storage_drill", c.id)
		end,
		ui.div {
			w = 26,
			h = 26,
			rounded = "$radius.md",
			bg = theme.alpha(c.color, 0.14),
			items = "center",
			justify = "center",
			icons.icon(c.icon, 14, c.color),
		},
		ui.text { c.label, grow = 1, min_w = 0, truncate = true },
		ui.text {
			function()
				return data().text
			end,
			font = "$font.mono",
			text_size = 12.5,
			color = "$text.secondary",
		},
		ui.text {
			function()
				local share = data().share or 0
				return if share < 0.001 then "—" else string.format("%d%%", math.max(1, math.floor(share * 100 + 0.5)))
			end,
			font = "$font.mono",
			text_size = 11,
			w = 34,
			align = "end",
			color = "$text.tertiary",
		},
	}
end

local function legend(): Node
	local items = {}
	for i, c in theme.categories do
		table.insert(items, legend_item(c, i))
	end
	return ui.grid { cols = 3, gap_x = 6, gap_y = 0, w_full = true, table.unpack(items) }
end

local function hero(): Node
	return common.card {
		w_full = true,
		p = 24,
		gap = 18,
		ui.row {
			w_full = true,
			items = "start",
			gap = 12,
			ui.column {
				grow = 1,
				min_w = 0,
				gap = 10,
				ui.row {
					gap = 10,
					items = "center",
					ui.text {
						function()
							return `STORAGE · {storage().selected or "C:"}`
						end,
						font = "$font.mono",
						text_size = 10.5,
						letter_spacing = 1.2,
						color = "$text.tertiary",
					},
					mode_badge(),
				},
				readout(),
				ui.text {
					field("status"),
					literal = true,
					font = "$font.mono",
					text_size = 12,
					color = "$text.tertiary",
					truncate = true,
					w_full = true,
				},
			},
			rescan_button(),
		},
		ui.column {
			w_full = true,
			gap = 8,
			meter.meter {
				data = function()
					local s = storage()
					return { bytes = s.bytes or {}, total = s.total or 1 }
				end,
				scanning = function()
					return storage().phase == "scanning"
				end,
				highlight = highlighted,
				glow = glow,
				motion = motion,
				dot = 7,
				gap = 3,
				w_full = true,
				h = 108,
				label = function()
					local s = storage()
					return `{s.used_text or ""} of {s.total_text or ""} used`
				end,
			},
			ui.row {
				w_full = true,
				ui.text {
					function()
						return if storage().phase == "scanning" then `{math.floor(storage().files or 0)} files so far` else ""
					end,
					font = "$font.mono",
					text_size = 11,
					color = "$text.tertiary",
					grow = 1,
				},
				ui.text {
					function()
						local s = storage()
						return if s.free_text then `{s.free_text} free` else ""
					end,
					font = "$font.mono",
					text_size = 11,
					color = "$text.tertiary",
				},
			},
		},
		ui.div { h = 1, w_full = true, bg = "$border.subtle" },
		legend(),
	}
end

-- Drill-down ------------------------------------------------------------------

local function drill_panel(): Node
	local items = ui.computed(function()
		local d = storage().drill
		return if d then d.items else {}
	end)
	return ui.show(
		function()
			return storage().drill ~= nil
		end,
		common.card {
			w_full = true,
			p = 20,
			gap = 12,
			-- Opening it brings it into view (it sits below the hero).
			focusable = true,
			label = "Biggest places",
			on_mount = function(el)
				el:focus()
			end,
			ui.row {
				w_full = true,
				gap = 10,
				items = "center",
				ui.text {
					function()
						local d = storage().drill
						local m = if d then category_meta(d.id) else nil
						return if m then `Biggest in {m.label}` else ""
					end,
					weight = 600,
					grow = 1,
				},
				common.icon_button { label = "Close", icon = "close", on_click = ui.action("storage_drill", nil :: any), size = 28 },
			},
			ui.each(items, function(item: any, index: number)
				local d = storage().drill
				local colour = if d then category_meta(d.id).color else "#FFB020"
				return ui.row {
					role = "button",
					label = `{item.path}, {item.text}`,
					w_full = true,
					gap = 12,
					items = "center",
					px = 8,
					py = 7,
					rounded = "$radius.md",
					hover = { bg = "$hover" },
					on_click = function()
						ui.invoke("storage_open", item.path)
					end,
					ui.text { string.format("%02d", index), font = "$font.mono", text_size = 11, color = "$text.tertiary", w = 22 },
					ui.column {
						grow = 1,
						min_w = 0,
						gap = 5,
						ui.row {
							w_full = true,
							gap = 8,
							ui.text { item.name, literal = true, weight = 500, truncate = true, min_w = 0, grow = 1 },
							ui.text { item.text, literal = true, font = "$font.mono", text_size = 12, color = "$text.secondary" },
						},
						ui.div {
							w_full = true,
							h = 3,
							rounded = "$radius.full",
							bg = "$app.led_off",
							ui.div {
								h_full = true,
								w = `{math.max(1, math.floor(item.bar * 100))}%`,
								rounded = "$radius.full",
								bg = colour,
							},
						},
						ui.text {
							short_path(item.path, 96),
							literal = true,
							font = "$font.mono",
							text_size = 10.5,
							color = "$text.tertiary",
							truncate = true,
						},
					},
					ui.div { color = "$text.tertiary", icons.icon("external", 14) },
				}
			end),
		}
	)
end

-- Cleanup -----------------------------------------------------------------------

local CLEANUP = {
	recycle_bin = { title = "Recycle Bin", icon = "trash", colour = "#FF5D4D", hint = "Deleted files still using space." },
	temp = { title = "Temporary files", icon = "broom", colour = "#E8D35A", hint = "Left behind by apps and installers." },
	caches = { title = "App & browser caches", icon = "bolt", colour = "#FFB020", hint = "Rebuilt automatically when needed." },
	large_files = { title = "Large files", icon = "sparkle", colour = "#C58CFF", hint = "Files over 1 GB." },
	old_downloads = { title = "Old downloads", icon = "download", colour = "#7FA8FF", hint = "Untouched for 90+ days." },
	dev_builds = { title = "Build folders", icon = "code", colour = "#5EC8FF", hint = "node_modules and target folders." },
}

local function cleanup_card(c: any): Node
	local meta = CLEANUP[c.id] or { title = c.id, icon = "dots", colour = "#888888", hint = "" }
	local empty = c.bytes <= 0
	local top = c.items and c.items[1]
	return common.card {
		p = 16,
		gap = 10,
		grow = 1,
		basis = 280,
		min_w = 240,
		opacity = if empty then 0.55 else 1,
		ui.row {
			w_full = true,
			gap = 10,
			items = "center",
			ui.div {
				w = 30,
				h = 30,
				rounded = "$radius.md",
				bg = theme.alpha(meta.colour, 0.14),
				items = "center",
				justify = "center",
				icons.icon(meta.icon, 16, meta.colour),
			},
			ui.column {
				grow = 1,
				min_w = 0,
				gap = 1,
				ui.text { meta.title, weight = 600, truncate = true },
				ui.text { meta.hint, color = "$text.tertiary", text_size = 11.5, truncate = true },
			},
		},
		ui.row {
			items = "end",
			gap = 8,
			ui.text { if empty then "Nothing" else c.text, font = "$font.mono", text_size = 22, weight = 500 },
			ui.text {
				if empty then "to clear" else `in {c.count_text} files`,
				color = "$text.tertiary",
				text_size = 12,
				pb = 3,
			},
		},
		ui.text {
			if top then short_path(top.path, 52) else " ",
			literal = true,
			font = "$font.mono",
			text_size = 10.5,
			color = "$text.tertiary",
			truncate = true,
			w_full = true,
		},
		ui.row {
			gap = 6,
			ui.button {
				label = `Review {meta.title}`,
				disabled = empty,
				on_click = ui.action("cleanup_review", c.id),
				px = 12,
				h = 28,
				rounded = "$radius.md",
				bg = "$surface.sunken",
				border = 1,
				border_color = "$border.subtle",
				hover = { border_color = "$border.strong" },
				ui.text { "Review", text_size = 12.5 },
			},
			ui.show(
				c.id == "recycle_bin" and not empty,
				ui.button {
					label = "Empty Recycle Bin",
					on_click = ui.action("empty_recycle_bin"),
					px = 12,
					h = 28,
					rounded = "$radius.md",
					bg = theme.alpha("#FF5D4D", 0.14),
					color = "$danger",
					hover = { bg = theme.alpha("#FF5D4D", 0.22) },
					ui.text { "Empty…", text_size = 12.5 },
				}
			),
			-- Temporary files: cleared for good (moving them to the Recycle
			-- Bin would free nothing), after asking.
			ui.show(
				c.id == "temp" and not empty,
				ui.button {
					label = "Clear temporary files",
					on_click = function()
						confirm.ask {
							title = "Clear temporary files?",
							body = `About {c.text} of temporary files will be deleted for good. Files apps are using right now are skipped. This can't be undone.`,
							label = "Clear",
							danger = true,
							action = "clean_temp",
							arg = { confirmed = true },
						}
					end,
					px = 12,
					h = 28,
					rounded = "$radius.md",
					bg = theme.alpha("#FF5D4D", 0.14),
					color = "$danger",
					hover = { bg = theme.alpha("#FF5D4D", 0.22) },
					ui.text { "Clear…", text_size = 12.5 },
				}
			),
		},
	}
end

local function cleanup_section(): Node
	local cards = ui.computed(function()
		return storage().cleanup or {}
	end)
	return ui.show(
		function()
			return #cards:get() > 0
		end,
		ui.column {
			w_full = true,
			gap = 12,
			ui.row {
				w_full = true,
				items = "center",
				gap = 10,
				common.caption("Things you can clear"),
				ui.div { grow = 1, h = 1, bg = "$border.subtle" },
			},
			ui.row {
				flex_wrap = true,
				gap = 12,
				w_full = true,
				ui.each(cards, function(c: any)
					return cleanup_card(c)
				end),
			},
		}
	)
end

-- Instant scan banner -------------------------------------------------------------

local function instant_banner(): Node
	return ui.show(
		function()
			local s = storage()
			return s.phase ~= nil and not s.instant and s.can_install == true
		end,
		ui.row {
			w_full = true,
			gap = 14,
			items = "center",
			px = 16,
			py = 12,
			rounded = "$radius.lg",
			bg = "$selection.item",
			border = 1,
			border_color = "$focus.ring",
			ui.div {
				w = 30,
				h = 30,
				rounded = "$radius.md",
				bg = "$accent",
				shadow = { x = 0, y = 0, blur = 14, spread = 0, color = "$app.glow" },
				items = "center",
				justify = "center",
				icons.icon("bolt", 16, "$accent.text"),
			},
			ui.column {
				grow = 1,
				min_w = 0,
				gap = 2,
				ui.text { "Make scans instant", weight = 600 },
				ui.text {
					"Pane can read your drive's index directly: an exact picture in seconds. Windows asks for permission once.",
					color = "$text.secondary",
					text_size = 12.5,
					wrap = true,
				},
			},
			ui.button {
				label = "Enable instant scan",
				on_click = ui.action("enable_instant_scan"),
				px = 14,
				h = 32,
				rounded = "$radius.md",
				bg = "$accent",
				color = "$accent.text",
				hover = { bg = "$accent.hover" },
				shrink = 0,
				ui.text { "Enable instant scan", weight = 600, text_size = 12.5 },
			},
		}
	)
end

-- Messages from the Overview (install results) as toasts.
local last_toast = 0
ui.effect(function()
	local t = (state.storage:get() or {}).toast
	if t and t.at ~= last_toast then
		last_toast = t.at
		ui.toast { title = t.message, duration = 5 }
	end
end)

-- The page --------------------------------------------------------------------

local function first_run(): Node
	return ui.show(
		function()
			return storage().phase == "empty"
		end,
		ui.column {
			w_full = true,
			items = "center",
			gap = 8,
			py = 40,
			ui.text { "Let's see what's using your space", text_size = 18, weight = 600 },
			ui.text { "Pane scans your drive once, then keeps the picture up to date.", color = "$text.secondary" },
		}
	)
end

return function(): Node
	return ui.scroll {
		grow = 1,
		w_full = true,
		min_h = 0,
		scrollbar = true,
		bg_shader = backdrop,
		-- Grid tint: warm amber, faint.
		shader_params = { { 1.0, 0.69, 0.13, 0.05 } },
		ui.column {
			w_full = true,
			max_w = 1080,
			self = "center",
			px = 28,
			py = 24,
			gap = 18,
			drives_row(),
			instant_banner(),
			hero(),
			drill_panel(),
			first_run(),
			cleanup_section(),
		},
	}
end
