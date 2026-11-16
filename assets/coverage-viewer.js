// {impl}`requirements.test-coverage.index.details`
// Sources are shared across report chapters; counters remain specific to each process.
function habeasRenderCoverage(config) {
  const root = document.getElementById(config.target);
  const frame = document.createElement("iframe");
  frame.hidden = true;
  frame.title = "Captured coverage source";
  const loading = document.createElement("p");
  loading.textContent = "Loading captured source…";
  root.append(loading);

  const element = (tag, text) => {
    const node = document.createElement(tag);
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const received = (event) => {
    if (event.source !== frame.contentWindow ||
        event.data?.kind !== "habeas-coverage-source" ||
        event.data.run !== config.run) return;
    window.removeEventListener("message", received);
    frame.remove();
    loading.remove();
    const sources = event.data.sources;
    for (const process of config.processes) {
      root.append(element("h2", `${process.kind} process ${process.pid}`));
      root.append(element("p", `Profiles: ${process.profiles.join(", ")}`));
      if (process.diagnostic) {
        root.append(element("p", `Unavailable: ${process.diagnostic}`));
      }
      if (Object.keys(process.lines).length === 0) {
        root.append(element("p", "No instrumented first-party Rust lines are available."));
      }
      let opened = false;
      for (const [file, lines] of Object.entries(process.lines)) {
        const rows = Object.entries(lines).sort((a, b) => Number(a[0]) - Number(b[0]));
        const covered = rows.filter(([, count]) => count !== "0").length;
        const details = element("details");
        const summary = element("summary", `${file}: ${covered}/${rows.length} lines executed`);
        details.append(summary);
        let rendered = false;
        const render = () => {
          if (rendered || !details.open) return;
          rendered = true;
          if (!Object.hasOwn(sources, file)) {
            details.append(element("p", "Unavailable: matching captured source is missing."));
            return;
          }
          const source = sources[file].split("\n");
          const table = element("table");
          const head = element("tr");
          for (const title of ["Line", "Executions", "Captured source"]) {
            head.append(element("th", title));
          }
          const thead = element("thead");
          thead.append(head);
          table.append(thead);
          const body = element("tbody");
          for (const [line, count] of rows) {
            const row = element("tr");
            row.append(element("td", line));
            // Counter strings preserve the full u64 value, beyond JS number precision.
            row.append(element("td", `${count} ${count === "0" ? "unexecuted" : "executed"}`));
            const cell = element("td");
            cell.append(element("pre", (source[Number(line) - 1] ??
              "(captured line unavailable)").replace(/\r$/, "")));
            row.append(cell);
            body.append(row);
          }
          table.append(body);
          details.append(table);
        };
        details.addEventListener("toggle", render);
        root.append(details);
        if (!opened && covered > 0) {
          opened = true;
          details.open = true;
          render();
        }
      }
    }
  };
  window.addEventListener("message", received);
  frame.src = config.source;
  root.append(frame);
}
