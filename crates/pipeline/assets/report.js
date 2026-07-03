// inspectah HTML audit report — progressive enhancement
// Provides table filtering, TOC navigation, and print support.
// Degrades gracefully: report is fully usable without JS.

document.addEventListener("DOMContentLoaded", function () {
  // ── Table filtering ─────────────────────────────────────────
  var dataEl = document.getElementById("report-filter-data");
  if (dataEl) {
    var filterData;
    try {
      filterData = JSON.parse(dataEl.textContent);
    } catch (e) {
      filterData = null;
    }

    if (filterData) {
      document.querySelectorAll(".report-filter input").forEach(function (input) {
        input.addEventListener("input", function () {
          var section = input.getAttribute("data-section");
          // Map HTML data-section names to JSON keys
          var jsonKey = section.replace(/-/g, "_");
          if (jsonKey === "scheduled_tasks") jsonKey = "scheduled";
          if (jsonKey === "users_groups") jsonKey = "users";
          var items = filterData[jsonKey];
          if (!items) return;

          var query = input.value.toLowerCase().trim();
          var table = document.querySelector('[data-filterable="' + section + '"]');
          if (!table) return;

          var rows = table.querySelectorAll("tbody tr");
          var shown = 0;
          var total = items.length;

          rows.forEach(function (row, i) {
            if (i >= total) return;
            var fields = items[i];
            var match = !query || Object.values(fields).some(function (v) {
              return String(v).toLowerCase().indexOf(query) !== -1;
            });
            row.style.display = match ? "" : "none";
            if (match) shown++;
          });

          // Update count display
          var countEl = input.closest(".report-filter").querySelector(".filter-count");
          if (countEl) {
            countEl.textContent = "Showing " + shown + " of " + total;
          }

          // No-results message
          var noResults = table.parentElement.querySelector(".filter-no-results");
          if (shown === 0 && query) {
            if (!noResults) {
              noResults = document.createElement("p");
              noResults.className = "filter-no-results";
              noResults.setAttribute("aria-live", "polite");
              table.parentElement.appendChild(noResults);
            }
            noResults.textContent = "No matching items";
            noResults.style.display = "";
          } else if (noResults) {
            noResults.style.display = "none";
          }
        });
      });
    }
  }

  // ── Section group disclosure ──────────────────────────────────
  document.querySelectorAll(".section-group__toggle").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var expanded = btn.getAttribute("aria-expanded") === "true";
      btn.setAttribute("aria-expanded", String(!expanded));
      var content = document.getElementById(btn.getAttribute("aria-controls"));
      if (content) content.hidden = expanded;
      var summary = btn.parentElement.querySelector(".section-group__collapsed-summary");
      if (summary) summary.hidden = !expanded;
      if (!expanded && content) {
        var firstItem = content.querySelector("summary, [tabindex='0'], button");
        if (firstItem) firstItem.focus();
      }
    });
  });

  // ── TOC navigation ──────────────────────────────────────────
  function openHashTarget() {
    var hash = window.location.hash;
    if (!hash) return;
    var target = document.querySelector(hash);
    if (!target) return;
    // Expand containing section group if collapsed
    var groupContent = target.closest(".section-group__content");
    if (groupContent && groupContent.hidden) {
      groupContent.hidden = false;
      var toggle = groupContent.parentElement.querySelector(".section-group__toggle");
      if (toggle) {
        toggle.setAttribute("aria-expanded", "true");
        var groupSummary = toggle.parentElement.querySelector(".section-group__collapsed-summary");
        if (groupSummary) groupSummary.hidden = true;
      }
    }
    var details = target.closest("details");
    if (details) {
      details.open = true;
      var summary = details.querySelector("summary");
      if (summary) summary.focus();
    }
    target.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  openHashTarget();
  window.addEventListener("hashchange", openHashTarget);

  // ── Print support ───────────────────────────────────────────
  var savedStates = [];

  window.addEventListener("beforeprint", function () {
    savedStates = [];
    // Save and expand section groups
    document.querySelectorAll(".section-group__toggle").forEach(function (btn) {
      var content = document.getElementById(btn.getAttribute("aria-controls"));
      savedStates.push({
        el: btn,
        kind: "group",
        wasExpanded: btn.getAttribute("aria-expanded") === "true"
      });
      btn.setAttribute("aria-expanded", "true");
      if (content) content.hidden = false;
    });
    // Save and expand details sections
    document.querySelectorAll("details").forEach(function (d) {
      savedStates.push({ el: d, kind: "details", wasOpen: d.open });
      d.open = true;
    });
  });

  window.addEventListener("afterprint", function () {
    savedStates.forEach(function (s) {
      if (s.kind === "group") {
        s.el.setAttribute("aria-expanded", String(s.wasExpanded));
        var content = document.getElementById(s.el.getAttribute("aria-controls"));
        if (content) content.hidden = !s.wasExpanded;
      } else if (s.kind === "details") {
        s.el.open = s.wasOpen;
      }
    });
    savedStates = [];
  });
});
