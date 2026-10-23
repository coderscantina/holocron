const $ = (selector) => document.querySelector(selector);
const icon = (name) => `<svg class="icon" aria-hidden="true"><use href="#i-${name}"/></svg>`;
const pages = [
  ["/de/events", 459, 34],
  ["/de", 346, 26],
  ["/de/events/33rd-roadhouse-birthday-party-0d3vyl", 134, 10],
  ["/de/events/25-jahre-revivalband-brtrvv", 111, 8],
  ["/en", 57, 4],
  ["/de/events/krissy-matthews-band-uk-no-feat-jenn-nettles-uk-rf4wiz", 52, 4],
  ["/de/checkout", 50, 4],
  ["/de/events/xandria-ger-seven-spires-usa-tulip-usa-auedjs", 44, 3],
  ["/de/checkout/payment", 43, 3],
  ["/de/events/my-son-the-doctor-us-4ihk7a", 41, 3],
];
const referrers = [
  ["roadhouse.test", 506, 42, "R"],
  ["google.com", 483, 40, "G"],
  ["instagram.com", 103, 8, "I"],
  ["m.facebook.com", 30, 2, "f"],
  ["5z604.r.sp1-brevo.net", 24, 2, "B"],
  ["bing.com", 21, 2, "b"],
  ["l.instagram.com", 16, 1, "I"],
  ["duckduckgo.com", 13, 1, "D"],
  ["facebook.com", 12, 1, "f"],
  ["ecosia.org", 11, 1, "E"],
];
const datasets = {
  page: pages,
  referrer: referrers,
  browser: [
    ["Chrome", 423, 39, "C"],
    ["iOS", 229, 21, "S"],
    ["Firefox", 151, 14, "F"],
    ["Safari", 140, 13, "S"],
  ],
  os: [
    ["iOS", 420, 38, "i"],
    ["Android", 414, 37, "A"],
    ["Windows", 171, 15, "W"],
    ["macOS", 106, 10, "M"],
  ],
  device: [
    ["Mobile", 816, 74, "M"],
    ["Desktop", 140, 13, "D"],
    ["Tablet", 151, 13, "T"],
  ],
};
const hourly = [
  112, 76, 34, 25, 24, 45, 12, 18, 60, 110, 172, 187, 306, 294, 200, 196, 207, 187, 286, 247, 241,
  223, 307, 48,
];
const escapeHtml = (value) =>
  String(value).replace(
    /[&<>"']/g,
    (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[char],
  );
const labels = {
  device: "Device",
  page: "Page",
  referrer: "Referrer",
  browser: "Browser",
  os: "Operating system",
};
let toastTimer;
let savedViews = [];
try {
  savedViews = JSON.parse(localStorage.getItem("holocron-example-views") || "[]");
  if (!Array.isArray(savedViews)) savedViews = [];
} catch {
  savedViews = [];
}
function readState() {
  const params = new URLSearchParams(location.search);
  let filters = [];
  try {
    filters = JSON.parse(params.get("filters") || "[]");
    if (!Array.isArray(filters)) filters = [];
    filters = filters.filter(
      (f) =>
        f &&
        typeof f.field === "string" &&
        typeof f.value === "string" &&
        ["eq", "neq", "contains"].includes(f.operator),
    );
  } catch {}
  return {
    range: ["24h", "7d", "30d"].includes(params.get("range")) ? params.get("range") : "24h",
    metric: ["views", "sessions", "visitors", "bounce", "duration"].includes(params.get("metric"))
      ? params.get("metric")
      : "views",
    compare: params.get("compare") !== "off",
    interval: params.get("interval") === "Daily" ? "Daily" : "Hourly",
    filters,
  };
}
function writeState(patch) {
  const state = { ...readState(), ...patch };
  const params = new URLSearchParams();
  if (state.range !== "24h") params.set("range", state.range);
  if (state.metric !== "views") params.set("metric", state.metric);
  if (!state.compare) params.set("compare", "off");
  if (state.interval !== "Hourly") params.set("interval", state.interval);
  if (state.filters.length) params.set("filters", JSON.stringify(state.filters));
  history.pushState(null, "", `${location.pathname}${params.size ? "?" + params : ""}`);
  render();
}
function toast(message) {
  clearTimeout(toastTimer);
  $("#toast").textContent = message;
  $("#toast").hidden = false;
  toastTimer = setTimeout(() => {
    $("#toast").hidden = true;
  }, 4200);
}
function renderTable(target, field, rows, compact = false) {
  $(target).innerHTML =
    `<table><thead><tr><th scope="col">${compact ? labels[field] || field : field === "page" ? "Page" : "Source"}</th><th scope="col">Visitor-days</th></tr></thead><tbody>${rows.map(([name, count, percent, logo]) => `<tr><td><button class="row-link" data-field="${field}" data-value="${escapeHtml(name)}" title="Filter by ${escapeHtml(name)}">${logo ? `<span class="row-logo" aria-hidden="true">${escapeHtml(logo)}</span>` : ""}<span class="row-name">${escapeHtml(name)}</span></button></td><td><span class="row-bar" style="width:${percent}%" aria-hidden="true"></span><span class="row-value">${count}</span><span class="row-percent">${percent}%</span></td></tr>`).join("")}</tbody></table>`;
}
function renderChart(state) {
  const compact = matchMedia("(max-width: 650px)").matches;
  const width = compact ? 340 : 1100,
    height = compact ? 210 : 260,
    left = compact ? 28 : 36,
    top = 16,
    bottom = 30,
    plotHeight = height - top - bottom,
    plotWidth = width - left - 5;
  const names = {
    views: "Views",
    sessions: "Sessions",
    visitors: "Visitor-days",
    bounce: "Bounce rate",
    duration: "Observed duration",
  };
  const scale = { views: 1, sessions: 0.34, visitors: 0.31, bounce: 0.16, duration: 0.4 }[
    state.metric
  ];
  const maximum = state.metric === "views" ? 350 : state.metric === "bounce" ? 60 : 150;
  const series =
    state.interval === "Daily"
      ? Array.from(
          { length: 7 },
          (_, i) => hourly.slice(i * 3, i * 3 + 3).reduce((a, b) => a + b, 0) / 3,
        )
      : hourly;
  const step = plotWidth / series.length;
  let svg = `<svg viewBox="0 0 ${width} ${height}" aria-hidden="true">`;
  for (let i = 0; i <= 5; i++) {
    const value = (maximum * i) / 5,
      y = height - bottom - (plotHeight * i) / 5;
    svg += `<line class="grid-line" x1="${left}" x2="${width}" y1="${y}" y2="${y}"/><text x="${left - 9}" y="${y + 3}" text-anchor="end">${Math.round(value)}${state.metric === "bounce" ? "%" : ""}</text>`;
  }
  const previous = [];
  series.forEach((raw, i) => {
    const value = raw * scale,
      x = left + i * step + step * 0.19,
      y = height - bottom - Math.min(value / maximum, 1) * plotHeight,
      w = step * 0.62;
    const visitors = raw * 0.31;
    svg += `<g class="hour-group"><title>${state.interval === "Daily" ? `Day ${i + 1}` : `${(22 + i) % 24}:00`}: ${Math.round(value)} ${names[state.metric].toLowerCase()} (sample)</title><rect class="view-bar" x="${x}" y="${y}" width="${w}" height="${height - bottom - y}" rx="2"/>`;
    if (state.metric === "views") {
      const vy = height - bottom - (visitors / maximum) * plotHeight;
      svg += `<rect class="visitor-bar" x="${x}" y="${vy}" width="${w}" height="${height - bottom - vy}" rx="2"/>`;
    }
    svg += `<rect class="hit-area" x="${x}" y="${top}" width="${w}" height="${plotHeight}"/>`;
    if (series.length <= 7 || i % (compact ? 4 : 2) === 0)
      svg += `<text x="${x + w / 2}" y="${height - 10}" text-anchor="middle">${state.interval === "Daily" ? ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][i] : `${(22 + i) % 24}:00`}</text>`;
    svg += "</g>";
    previous.push(
      `${x + w / 2},${height - bottom - Math.min((value * 0.83) / maximum, 1) * plotHeight}`,
    );
  });
  if (state.compare)
    svg += `<polyline class="previous-line" points="${previous.join(" ")}"><title>Previous period, illustrative series</title></polyline>`;
  svg += "</svg>";
  $("#chart").innerHTML = svg;
  $("#chart-legend").textContent = names[state.metric];
  $(".secondary-legend").hidden = state.metric !== "views";
  $("#chart-subtitle").textContent =
    state.metric === "views"
      ? `${state.interval === "Hourly" ? "Hourly" : "Daily"} views and daily-scoped visitor identities`
      : `${names[state.metric]} · illustrative example series`;
}
function render() {
  const state = readState();
  $("#date-range").value = state.range;
  $("#granularity").value = state.interval;
  $("#compare").setAttribute("aria-pressed", String(state.compare));
  document.querySelectorAll("[data-metric]").forEach((button) => {
    const selected = button.dataset.metric === state.metric;
    button.classList.toggle("selected", selected);
    button.setAttribute("aria-pressed", String(selected));
  });
  $("#filter-chips").innerHTML = state.filters
    .map(
      (f, i) =>
        `<span class="chip"><span>${escapeHtml(labels[f.field] || f.field)}</span><span>${f.operator === "neq" ? "is not" : f.operator === "contains" ? "contains" : "is"} ${escapeHtml(f.value)}</span><button data-remove="${i}" aria-label="Remove ${escapeHtml(labels[f.field] || f.field)} filter">${icon("x")}</button></span>`,
    )
    .join("");
  $("#clear-filters").hidden = !state.filters.length;
  $("#chart-title").textContent =
    state.range === "24h"
      ? "Traffic over time"
      : state.range === "7d"
        ? "Traffic · last 7 days"
        : "Traffic · last 30 days";
  renderChart(state);
}
function openDialog(selector) {
  $(selector).showModal();
}
function addFilter(field, value, operator = "eq") {
  writeState({ filters: [...readState().filters, { field, value, operator }] });
  toast("Filter added. Sample figures remain unchanged.");
}
function openBreakdown(field) {
  $("#detail-title").textContent = `Explore ${field === "page" ? "pages" : "referrers"}`;
  $("#detail-content").innerHTML =
    `<p class="detail-summary">${readState().filters.length} active filters · ${$("#date-range").selectedOptions[0].textContent}. Choose a row to drill down without losing your date range or comparison.</p><ul class="detail-list">${datasets[field].map(([name, count]) => `<li><button class="row-link" data-field="${field}" data-value="${escapeHtml(name)}"><span class="row-name">${escapeHtml(name)}</span></button><strong>${count}</strong></li>`).join("")}</ul>`;
  openDialog("#detail-dialog");
}
function renderSearch() {
  const needle = $("#command-input").value.toLowerCase();
  const matches = [
    ...pages.map((row) => ({ field: "page", name: row[0] })),
    ...referrers.map((row) => ({ field: "referrer", name: row[0] })),
  ]
    .filter((row) => row.name.toLowerCase().includes(needle))
    .slice(0, 12);
  $("#command-results").innerHTML = matches.length
    ? matches
        .map(
          (row) =>
            `<button class="command-result" data-field="${row.field}" data-value="${escapeHtml(row.name)}">${icon(row.field === "page" ? "chart" : "globe")}<span>${escapeHtml(row.name)}</span></button>`,
        )
        .join("")
    : '<p class="dialog-note">No sample results match.</p>';
}
function openSaved() {
  $("#saved-views").innerHTML = savedViews
    .map(
      (view, i) =>
        `<button type="button" class="saved-view-button" data-saved="${i}">${icon("bookmark")}${escapeHtml(view.name)}</button>`,
    )
    .join("");
  openDialog("#save-dialog");
}
document.addEventListener("click", (event) => {
  const menuAction = event.target.closest("[data-example-action], [data-menu-close], [data-theme]");
  if (menuAction) {
    const menu = menuAction.closest("[popover]");
    if (menuAction.dataset.theme) setTheme(menuAction.dataset.theme);
    if (menuAction.dataset.exampleAction)
      toast(`${menuAction.dataset.exampleAction} is a reference-screen action.`);
    menu.hidePopover();
    document.querySelector(`[popovertarget="${menu.id}"]`).focus();
    return;
  }
  const close = event.target.closest("[data-close]");
  if (close) {
    close.closest("dialog").close();
    return;
  }
  const remove = event.target.closest("[data-remove]");
  if (remove) {
    writeState({
      filters: readState().filters.filter((_, i) => i !== Number(remove.dataset.remove)),
    });
    return;
  }
  const row = event.target.closest("[data-field][data-value]");
  if (row) {
    const dialog = row.closest("dialog");
    if (dialog) dialog.close();
    addFilter(row.dataset.field, row.dataset.value);
    return;
  }
  const metric = event.target.closest("[data-metric]");
  if (metric) {
    writeState({ metric: metric.dataset.metric });
    return;
  }
  const more = event.target.closest("[data-more]");
  if (more) {
    openBreakdown(more.dataset.more);
    return;
  }
  const saved = event.target.closest("[data-saved]");
  if (saved) {
    const view = savedViews[Number(saved.dataset.saved)];
    writeState(view.state);
    $("#saved-label").textContent = view.name;
    $("#save-dialog").close();
    toast("Saved exploration restored.");
    return;
  }
  const nav = event.target.closest("[data-nav]");
  if (nav) {
    event.preventDefault();
    if (nav.dataset.nav === "overview") {
      document.documentElement.classList.remove("mobile-sidebar-open");
      return;
    }
    $("#detail-title").textContent = nav.textContent.trim();
    $("#detail-content").innerHTML =
      '<p class="detail-summary">This reference screen demonstrates the overview. Other product screens are part of the technical briefing.</p>';
    openDialog("#detail-dialog");
  }
});
$("#filter-open").onclick = () => openDialog("#filter-dialog");
$("#clear-filters").onclick = () => writeState({ filters: [] });
$("#date-range").onchange = (event) => {
  writeState({ range: event.target.value });
  toast("Date state updated. Values are illustrative sample data.");
};
$("#granularity").onchange = (event) => writeState({ interval: event.target.value });
$("#compare").onclick = () => writeState({ compare: !readState().compare });
$("#save-open").onclick = openSaved;
$("#mobile-save").onclick = openSaved;
$("#filter-form").onsubmit = (event) => {
  event.preventDefault();
  const data = new FormData(event.target);
  addFilter(data.get("field"), data.get("value").trim(), data.get("operator"));
  $("#filter-dialog").close();
};
$("#save-form").onsubmit = (event) => {
  event.preventDefault();
  const name = $("#view-name").value.trim();
  if (!name) return;
  const view = { name, state: readState() };
  savedViews = [...savedViews.filter((v) => v.name !== name), view];
  try {
    localStorage.setItem("holocron-example-views", JSON.stringify(savedViews));
  } catch {
    toast("Browser storage is unavailable; view kept for this session.");
  }
  $("#saved-label").textContent = name;
  $("#save-dialog").close();
  toast("Exploration saved in this browser.");
};
$("#command-open").onclick = () => {
  openDialog("#command-dialog");
  renderSearch();
  $("#command-input").focus();
};
$("#command-input").oninput = renderSearch;
let colorMode = "dark";
const systemTheme = matchMedia("(prefers-color-scheme: dark)");
function setTheme(mode) {
  colorMode = mode;
  document.documentElement.classList.toggle(
    "dark",
    mode === "dark" || (mode === "system" && systemTheme.matches),
  );
  document
    .querySelectorAll("[data-theme]")
    .forEach((button) =>
      button.setAttribute("aria-checked", String(button.dataset.theme === mode)),
    );
}
systemTheme.addEventListener("change", () => {
  if (colorMode === "system") setTheme("system");
});
document.querySelectorAll(".header-popover").forEach((menu) => {
  const trigger = document.querySelector(`[popovertarget="${menu.id}"]`);
  menu.addEventListener("toggle", (event) => {
    const open = event.newState === "open";
    trigger.setAttribute("aria-expanded", String(open));
    if (open) {
      const rect = trigger.getBoundingClientRect();
      const width = menu.offsetWidth;
      menu.style.top = `${rect.bottom + 6}px`;
      menu.style.left = `${Math.max(12, Math.min(menu.id === "user-menu" ? rect.right - width : rect.left, innerWidth - width - 12))}px`;
    }
  });
  trigger.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      menu.showPopover();
      menu.querySelector("button").focus();
    }
  });
  menu.addEventListener("keydown", (event) => {
    const items = [...menu.querySelectorAll("button")];
    const index = items.indexOf(document.activeElement);
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      const next =
        event.key === "Home"
          ? 0
          : event.key === "End"
            ? items.length - 1
            : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
      items[next].focus();
    }
    if (event.key === "Escape") {
      menu.hidePopover();
      trigger.focus();
    }
  });
});
function toggleSidebar() {
  const mobile = matchMedia("(max-width: 960px)").matches;
  const open = mobile
    ? document.documentElement.classList.toggle("mobile-sidebar-open")
    : !document.documentElement.classList.toggle("sidebar-collapsed");
  $("#sidebar-toggle").setAttribute("aria-expanded", String(open));
  $("#sidebar-toggle").setAttribute(
    "aria-label",
    open ? "Collapse navigation" : "Expand navigation",
  );
}
$("#sidebar-toggle").onclick = toggleSidebar;
$("#mobile-menu").onclick = toggleSidebar;
function openExplore() {
  const state = readState();
  $("#detail-title").textContent = "Current exploration";
  $("#detail-content").innerHTML =
    `<p class="detail-summary">The same state drives the URL, saved view, board card, and export. This is a local UX demonstration, without a backend.</p><ul class="detail-list"><li><span>Date range</span><strong>${$("#date-range").selectedOptions[0].textContent}</strong></li><li><span>Measure</span><strong>${escapeHtml(state.metric)}</strong></li><li><span>Active filters</span><strong>${state.filters.length}</strong></li><li><span>Comparison</span><strong>${state.compare ? "Previous period" : "Off"}</strong></li></ul><p class="dialog-note">Use browser Back to undo drill-downs. Clearing filters preserves the remaining exploration state.</p>`;
  openDialog("#detail-dialog");
}
$("#explore").onclick = openExplore;
$("#mobile-explore").onclick = openExplore;
$("#definitions").onclick = () => {
  $("#detail-title").textContent = "Metric definitions";
  $("#detail-content").innerHTML =
    '<p class="detail-summary"><strong>Visitor-days</strong> count daily-scoped pseudonymous identities, not unique people across the date range.</p><p class="detail-summary"><strong>Sessions</strong> use a configurable inactivity window and close at local midnight for anonymous traffic.</p><p class="detail-summary"><strong>Observed duration</strong> is the span between recorded session events. It does not measure continuous active time.</p><p class="dialog-note">Metric formulas are versioned. These screen values are samples from the supplied screenshot, not recalculated historical results.</p>';
  openDialog("#detail-dialog");
};
$("#settings").onclick = () =>
  toast("Example screen. Website settings are specified in the briefing.");
$("#export").onclick = () => {
  const state = readState();
  const csv = [
    "field,value,visitor_days,share_percent",
    ...pages.map(
      ([name, count, percent]) => `page,"${name.replaceAll('"', '""')}",${count},${percent}`,
    ),
    ...referrers.map(
      ([name, count, percent]) => `referrer,"${name.replaceAll('"', '""')}",${count},${percent}`,
    ),
  ].join("\r\n");
  const blob = new Blob(
    [`# Example data; range=${state.range}; filters=${JSON.stringify(state.filters)}\r\n${csv}`],
    { type: "text/csv" },
  );
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = "roadhouse-overview-example.csv";
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  toast("Example CSV exported with exploration state.");
};
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    $("#command-open").click();
  }
  if (event.key === "Escape") document.documentElement.classList.remove("mobile-sidebar-open");
});
window.addEventListener("popstate", render);
matchMedia("(max-width: 650px)").addEventListener("change", () => renderChart(readState()));
renderTable("#pages-table", "page", pages);
renderTable("#referrers-table", "referrer", referrers);
renderTable("#browsers-table", "browser", datasets.browser, true);
renderTable("#os-table", "os", datasets.os, true);
renderTable("#devices-table", "device", datasets.device, true);
render();
