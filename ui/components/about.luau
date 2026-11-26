-- The About dialog (More → About Pane): the version and "Copy version info",
-- "Check for updates", making Pane the default file manager, links, and the
-- open-source software Pane is built from.
-- Rust: crates/pane-app/src/about.rs (shared "about"), updates.rs.

local state = require("../state")
local icons = require("./icons")
local update_modal = require("./update_modal")
local licenses = require("../licenses")

local M = {}

M.open = ui.state("about_open", false)
local show_licenses = ui.state("about_licenses", false)
local copied = ui.state("about_copied", false)

local about = state.about
local checked = state.update_check

local function a(): any
	return about:get() or {}
end

function M.show()
	ui.invoke("about_refresh")
	show_licenses:set(false)
	M.open:set(true)
end

local function button(label: string, icon: string?, on_click: () -> (), props: { [string]: any }?): Node
	local p: { [string]: any } = {
		label = label,
		on_click = on_click,
		px = 12,
		h = 32,
		gap = 6,
		items = "center",
		rounded = "$radius.md",
		bg = "$surface.raised",
		border = 1,
		border_color = "$border.subtle",
		hover = { border_color = "$border.strong" },
		shrink = 0,
	}
	for k, v in props or {} do
		p[k] = v
	end
	local children: { any } = {}
	if icon then
		table.insert(children, icons.icon(icon, 14))
	end
	table.insert(children, ui.text { label })
	for i, c in children do
		p[i] = c
	end
	return ui.button(p)
end

local function section(title: string, body: { any }): Node
	local col: { [any]: any } = { w_full = true, gap = 10 }
	table.insert(col, ui.text {
		string.upper(title),
		font = "$font.mono",
		text_size = 10.5,
		letter_spacing = 1,
		color = "$text.tertiary",
	})
	for _, child in body do
		table.insert(col, child)
	end
	return ui.column(col)
end

local function version_info(): string
	local x = a()
	return `Pane {x.version or "?"} (x64)\n{x.windows or "Windows"}`
end

local function header(): Node
	return ui.row {
		w_full = true,
		gap = 14,
		items = "center",
		ui.image { src = "images/pane.png", w = 56, h = 56, shrink = 0, label = "Pane" },
		ui.column {
			grow = 1,
			min_w = 0,
			gap = 2,
			ui.text { "Pane", weight = 600, text_size = 20 },
			ui.text {
				function()
					return `Version {a().version or "?"}`
				end,
				font = "$font.mono",
				color = "$text.secondary",
				text_size = 12.5,
			},
			ui.text {
				function()
					return a().windows or ""
				end,
				color = "$text.tertiary",
				text_size = 12,
				truncate = true,
			},
		},
	}
end

local function update_row(): Node
	return ui.row {
		w_full = true,
		gap = 10,
		items = "center",
		ui.text {
			function()
				local c = checked:get()
				if c == "checking" then
					return "Checking for updates…"
				elseif c == "current" then
					return "Pane is up to date."
				elseif c == "available" then
					local u = state.update:get()
					return if u then `Pane {u.version} is available.` else "An update is available."
				elseif c == "error" then
					return "Couldn't reach GitHub. Check your connection."
				end
				return "Updates come from Pane's GitHub releases."
			end,
			grow = 1,
			min_w = 0,
			wrap = true,
			color = "$text.secondary",
			text_size = 13,
		},
		ui.show(function()
			return checked:get() == "available" and state.update:get() ~= nil
		end, button("Get the update", "download", function()
			M.open:set(false)
			update_modal.start()
		end), button("Check for updates", "refresh", function()
			ui.invoke("update_check")
		end, {
			disabled = function()
				return checked:get() == "checking"
			end,
		})),
	}
end

-- A switch: Pane opens folders instead of File Explorer.
local function default_switch(): Node
	local function on(): boolean
		return a().default_fm == true
	end
	return ui.button {
		role = "switch",
		label = "Open folders in Pane",
		checked = on,
		on_click = function()
			ui.invoke("set_default_fm", not on())
		end,
		w_full = true,
		p = 12,
		gap = 12,
		items = "center",
		rounded = "$radius.md",
		bg = "$surface.sunken",
		border = 1,
		border_color = "$border.subtle",
		hover = { border_color = "$border.strong" },
		ui.column {
			grow = 1,
			min_w = 0,
			gap = 3,
			items = "start",
			ui.text { "Open folders in Pane", weight = 600 },
			ui.text {
				"Folders, drives and Win+E open in Pane instead of File Explorer. Turn it off to give them back to the app that had them.",
				color = "$text.secondary",
				text_size = 12,
				wrap = true,
				w_full = true,
			},
		},
		-- The track and knob.
		ui.div {
			w = 40,
			h = 22,
			shrink = 0,
			rounded = "$radius.full",
			bg = function()
				return if on() then "$accent" else "$surface.raised"
			end,
			border = 1,
			border_color = function()
				return if on() then "$accent" else "$border.strong"
			end,
			shadow = function()
				return if on() then { x = 0, y = 0, blur = 10, spread = 0, color = "$app.glow" } else false
			end,
			relative = true,
			ui.div {
				absolute = true,
				top = 3,
				left = function()
					return if on() then 21 else 3
				end,
				w = 14,
				h = 14,
				rounded = "$radius.full",
				bg = function()
					return if on() then "$accent.text" else "$text.secondary"
				end,
			},
		},
	}
end

local function default_error(): Node
	return ui.show(function()
		return a().default_error ~= nil
	end, ui.text {
		function()
			return a().default_error or ""
		end,
		color = "$danger",
		text_size = 12,
		wrap = true,
	})
end

local function links(): Node
	local function link(label: string, page: string): Node
		return button(label, "external", function()
			ui.invoke("open_link", page)
		end, { px = 8, h = 30 })
	end
	return ui.row {
		w_full = true,
		gap = 6,
		wrap = true,
		link("GitHub", "repo"),
		link("Release notes", "releases"),
		link("Report an issue", "issues"),
		link("License", "license"),
	}
end

local function license_list(): Node
	return ui.list {
		count = #licenses,
		height = 24,
		h = 180,
		w_full = true,
		scrollbar = true,
		rounded = "$radius.md",
		bg = "$surface.sunken",
		border = 1,
		border_color = "$border.subtle",
		py = 4,
		row = function(i: number)
			local item = licenses[i]
			return ui.row {
				h = 24,
				w_full = true,
				px = 10,
				gap = 8,
				items = "center",
				ui.text { item.name, literal = true, grow = 1, min_w = 0, truncate = true, text_size = 12.5 },
				ui.text {
					item.version,
					literal = true,
					font = "$font.mono",
					text_size = 11.5,
					color = "$text.tertiary",
					shrink = 0,
				},
				ui.text {
					item.license,
					literal = true,
					w = 150,
					shrink = 0,
					truncate = true,
					text_size = 12,
					color = "$text.secondary",
				},
			}
		end,
	}
end

local function body(): Node
	return ui.column {
		w = 520,
		p = 24,
		gap = 20,
		bg = "$surface.overlay",
		rounded = "$radius.xl",
		border = 1,
		border_color = "$border.subtle",
		shadow = "$shadow.lg",
		inset_shadow = { x = 0, y = 1, color = "$app.highlight" },
		header(),
		ui.row {
			gap = 8,
			items = "center",
			button("Copy version info", "copy", function()
				ui.clipboard.write(version_info())
				copied:set(true)
				ui.after(2000, function()
					copied:set(false)
				end)
			end),
			ui.show(function()
				return copied:get()
			end, ui.text { "Copied", color = "$accent", text_size = 12.5 }),
		},
		section("Updates", { update_row() }),
		section("Default file manager", { default_switch(), default_error() }),
		section("Links", { links() }),
		section("Open-source software", {
			ui.text {
				`Pane is MIT-licensed and built on {#licenses} open-source projects.`,
				color = "$text.secondary",
				text_size = 12.5,
				wrap = true,
			},
			ui.show(function()
				return show_licenses:get()
			end, license_list(), button("Show licenses", nil, function()
				show_licenses:set(true)
			end)),
		}),
		ui.row {
			w_full = true,
			justify = "end",
			button("Close", nil, function()
				M.open:set(false)
			end),
		},
	}
end

function M.dialog(): Node
	return ui.modal {
		open = M.open,
		label = "About Pane",
		w = 520,
		p = 0,
		bg = "#00000000",
		build = body,
	}
end

return M
