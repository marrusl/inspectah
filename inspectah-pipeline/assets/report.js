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
          var items = filterData[section];
          if (!items) return;

          var query = input.value.toLowerCase().trim();
          var table = document.getElementById("table-" + section);
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

  // ── TOC navigation ──────────────────────────────────────────
  function openHashTarget() {
    var hash = window.location.hash;
    if (!hash) return;
    var target = document.querySelector(hash);
    if (!target) return;
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
    document.querySelectorAll("details.report-section").forEach(function (d) {
      savedStates.push({ el: d, wasOpen: d.open });
      d.open = true;
    });
  });

  window.addEventListener("afterprint", function () {
    savedStates.forEach(function (s) {
      s.el.open = s.wasOpen;
    });
    savedStates = [];
  });
});
