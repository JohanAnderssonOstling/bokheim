const wireMediaType = "application/vnd.bokheim+protobuf; version=13";
const elements = Object.fromEntries([...document.querySelectorAll("[id]")].map((element) => [element.id, element]));
let hours = 24;
let refreshing = false;
let refreshPromise;

const pageDefinitions = {
  overview: ["Operations overview", "/admin/"],
  traffic: ["Traffic", "/admin/traffic"],
  accounts: ["Account activity", "/admin/accounts"],
  catalogue: ["Catalogue operations", "/admin/catalogue"],
  metadata: ["Metadata enrichment", "/admin/metadata"],
  security: ["Security and incident response", "/admin/security"],
};
const requestedPage = window.location.pathname.replace(/^\/admin\/?/, "").split("/")[0] || "overview";
const activePage = Object.hasOwn(pageDefinitions, requestedPage) ? requestedPage : "overview";

function selectPage() {
  document.title = `${pageDefinitions[activePage][0]} · Bokheim`;
  elements["page-title"].textContent = pageDefinitions[activePage][0];
  for (const section of document.querySelectorAll("[data-page]")) section.hidden = section.dataset.page !== activePage;
  for (const link of document.querySelectorAll("[data-nav-page]")) {
    const active = link.dataset.navPage === activePage;
    link.classList.toggle("active", active);
    if (active) link.setAttribute("aria-current", "page"); else link.removeAttribute("aria-current");
  }
}

const number = (value) => new Intl.NumberFormat().format(value);
function bytes(value) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  return `${(value / 1024 ** index).toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}
function duration(seconds) {
  const days = Math.floor(seconds / 86400);
  const hour = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return days ? `${days}d ${hour}h` : hour ? `${hour}h ${minutes}m` : `${minutes}m`;
}
function timestamp(seconds) {
  return Number.isFinite(seconds) && seconds > 0 ? new Date(seconds * 1000).toLocaleString() : "—";
}
function perMinute(count, elapsedSeconds) {
  return Number.isFinite(count) && Number.isFinite(elapsedSeconds) && elapsedSeconds >= 2 ? count * 60 / elapsedSeconds : null;
}
function rate(value) {
  if (!Number.isFinite(value)) return "Measuring…";
  return new Intl.NumberFormat(undefined, { maximumFractionDigits:value < 10 ? 1 : 0 }).format(value);
}
function path(values) {
  if (!values.length) return "";
  const maximum = Math.max(...values, 1);
  return values.map((value, index) => {
    const x = values.length === 1 ? 360 : index / (values.length - 1) * 720;
    const y = 180 - value / maximum * 168 - 6;
    return `${index ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(" ");
}

function refreshSession() {
  if (!refreshPromise) {
    refreshPromise = fetch("/api/auth/web/refresh", { method:"POST", credentials:"same-origin", headers:{ Accept:wireMediaType } })
      .then((response) => response.ok)
      .finally(() => { refreshPromise = undefined; });
  }
  return refreshPromise;
}
async function request(pathname, retry = true) {
  let response = await fetch(pathname, { credentials:"same-origin", headers:{ Accept:"application/json" } });
  if (response.status === 401 && retry) {
    if (await refreshSession()) response = await fetch(pathname, { credentials:"same-origin", headers:{ Accept:"application/json" } });
  }
  return response;
}

function aggregate(points) {
  const buckets = new Map();
  const routes = new Map();
  for (const point of points) {
    const bucket = buckets.get(point.bucket_ms) ?? { at:point.bucket_ms, requests:0, errors:0, inbound:0, outbound:0 };
    bucket.requests += point.request_count;
    if (point.status_class >= 4) bucket.errors += point.request_count;
    bucket.inbound += point.request_bytes;
    bucket.outbound += point.response_bytes;
    buckets.set(point.bucket_ms, bucket);
    const route = routes.get(point.route_group) ?? { route:point.route_group, requests:0, errors:0, inbound:0, outbound:0, maxLatency:0 };
    route.requests += point.request_count;
    if (point.status_class >= 4) route.errors += point.request_count;
    route.inbound += point.request_bytes;
    route.outbound += point.response_bytes;
    route.maxLatency = Math.max(route.maxLatency, point.max_duration_ms);
    routes.set(point.route_group, route);
  }
  return {
    buckets:[...buckets.values()].sort((left, right) => left.at - right.at),
    routes:[...routes.values()].sort((left, right) => right.requests - left.requests || left.route.localeCompare(right.route)),
  };
}

function card(label, value, detail) {
  const item = document.createElement("article");
  item.className = "card";
  for (const [tag, text] of [["span", label], ["strong", value], ["small", detail]]) {
    const child = document.createElement(tag); child.textContent = text; item.append(child);
  }
  return item;
}
function serviceState(service) {
  return service.available ? ["Available", `${number(service.latency_ms)} ms response`] : ["Unavailable", "Check the service or dashboard target configuration"];
}
function render(overview, history, requestHistory, engagement, services) {
  const catalogueState = serviceState(services.catalogue);
  const metadataState = serviceState(services.metadata);
  elements.cards.replaceChildren(
    card("Requests · 1h", number(overview.requests_last_hour), `${number(overview.errors_last_hour)} errors`),
    card("Application API latency", `${overview.average_latency_ms_last_hour.toFixed(1)} ms`, `${number(overview.max_latency_ms_last_hour)} ms maximum · admin polling excluded`),
    card("Traffic · 1h", bytes(overview.request_bytes_last_hour + overview.response_bytes_last_hour), `↑ ${bytes(overview.request_bytes_last_hour)} · ↓ ${bytes(overview.response_bytes_last_hour)}`),
    card("Active sessions", number(overview.active_sessions), `${number(overview.active_websockets)} WebSockets`),
    card("In-flight requests", number(overview.in_flight_requests), `${number(overview.peak_in_flight_requests)} process peak`),
    card("Database pool", number(overview.database_connections), `${number(overview.database_idle_connections)} idle`),
    card("Active accounts · today", number(overview.daily_active_accounts), `${number(overview.weekly_active_accounts)} in 7d · ${number(overview.monthly_active_accounts)} in 30d`),
    card("New accounts · 30d", number(overview.new_verified_accounts_30d), `${number(overview.first_time_accounts_30d)} first-time · ${number(overview.returning_accounts_30d)} returning`),
    card("Sync accounts · 30d", number(overview.sync_accounts_30d), `${number(overview.auth_only_accounts_30d)} authenticated without sync`),
    card("Registered accounts", number(overview.registered_users), `${number(overview.unique_accounts_ever)} used server since tracking began`),
    card("Catalogue server", catalogueState[0], catalogueState[1]),
    card("Metadata enrichment", metadataState[0], metadataState[1]),
    card("Storage used", bytes(overview.storage_used_bytes), `${bytes(overview.storage_reserved_bytes)} reserved`),
    card("Process uptime", duration(overview.uptime_seconds), overview.dropped_traffic_events ? `${number(overview.dropped_traffic_events)} metrics dropped` : "No dropped metrics"),
  );
  const values = aggregate(history.points);
  elements["requests-path"].setAttribute("d", path(values.buckets.map((bucket) => bucket.requests)));
  elements["errors-path"].setAttribute("d", path(values.buckets.map((bucket) => bucket.errors)));
  elements["bytes-path"].setAttribute("d", path(values.buckets.map((bucket) => bucket.inbound + bucket.outbound)));
  elements["empty-chart"].hidden = values.buckets.length !== 0;
  elements["request-chart"].hidden = values.buckets.length === 0;
  elements["chart-period"].textContent = values.buckets.length ? `${new Date(values.buckets[0].at).toLocaleString()} — ${new Date(values.buckets.at(-1).at).toLocaleString()}` : "";
  elements["transfer-total"].textContent = bytes(values.buckets.reduce((sum, bucket) => sum + bucket.inbound + bucket.outbound, 0));
  const rows = values.routes.map((route) => {
    const row = document.createElement("tr");
    for (const [index, value] of [route.route, number(route.requests), number(route.errors), bytes(route.inbound), bytes(route.outbound), `${number(route.maxLatency)} ms`].entries()) {
      const cell = document.createElement("td");
      if (index === 0) { const code = document.createElement("code"); code.textContent = value; cell.append(code); } else cell.textContent = value;
      row.append(cell);
    }
    return row;
  });
  elements.routes.replaceChildren(...rows);
  const engagementRows = [...engagement.points].reverse().map((point) => {
    const row = document.createElement("tr");
    const values = [new Date(`${point.date}T00:00:00Z`).toLocaleDateString(), number(point.active_accounts), number(point.sync_accounts), number(point.new_verified_accounts)];
    for (const value of values) {
      const cell = document.createElement("td"); cell.textContent = value; row.append(cell);
    }
    return row;
  });
  elements.engagement.replaceChildren(...engagementRows);

  const catalogue = services.catalogue.data;
  elements["catalogue-unavailable"].hidden = Boolean(catalogue);
  elements["catalogue-content"].hidden = !catalogue;
  if (catalogue) {
    const extractions = catalogue.epub_extractions;
    const metadataBackfill = catalogue.metadata_backfill;
    const currentCrawls = catalogue.sources.filter((source) => source.crawl_status === "running");
    const crawlRates = currentCrawls.map((source) => ({
      pages:perMinute(source.crawl_pages_fetched, source.crawl_elapsed_seconds),
      publications:perMinute(source.crawl_publications_seen, source.crawl_elapsed_seconds),
    }));
    const measuredRates = crawlRates.filter((speed) => speed.pages !== null);
    const pagesPerMinute = measuredRates.reduce((sum, speed) => sum + speed.pages, 0);
    const publicationsPerMinute = measuredRates.reduce((sum, speed) => sum + speed.publications, 0);
    elements["catalogue-cards"].replaceChildren(
      card("Publications", number(catalogue.active_publication_count), `${number(catalogue.active_page_count)} cached pages`),
      card("Importable EPUBs", number(catalogue.active_epub_count), `${number(catalogue.sources.length)} configured sources`),
      card("Catalogue latency", `${number(services.catalogue.latency_ms)} ms`, "Operations snapshot response"),
      card("EPUB extraction queue", number(extractions.pending + extractions.running), `${number(extractions.succeeded)} succeeded · ${number(extractions.failed)} failed`),
      ...(metadataBackfill ? [card("Metadata backfill queue", number(metadataBackfill.pending + metadataBackfill.running), `${number(metadataBackfill.running)} running · ${number(metadataBackfill.succeeded)} succeeded`)] : []),
      card("Registry revision", number(catalogue.registry_revision), catalogue.metadata_enrichment_configured ? "Metadata enrichment configured" : "Metadata enrichment disabled"),
      card("Crawl speed", currentCrawls.length ? `${rate(measuredRates.length ? pagesPerMinute : null)} pages/min` : "Idle", currentCrawls.length ? `${rate(measuredRates.length ? publicationsPerMinute : null)} publication entries/min across ${number(currentCrawls.length)} sources` : "No source is currently crawling"),
    );
    elements["catalogue-metadata-backfill"].hidden = !metadataBackfill;
    if (metadataBackfill) {
      const row = document.createElement("tr");
      for (const value of [metadataBackfill.pending, metadataBackfill.running, metadataBackfill.succeeded, metadataBackfill.last_error || "None"]) {
        const cell = document.createElement("td");
        cell.textContent = typeof value === "number" ? number(value) : value;
        row.append(cell);
      }
      elements["catalogue-metadata-backfill-status"].replaceChildren(row);
    }
    elements["catalogue-current-crawl-empty"].hidden = currentCrawls.length !== 0;
    elements["catalogue-current-crawl-wrap"].hidden = currentCrawls.length === 0;
    const currentCrawlRows = currentCrawls.map((source, sourceIndex) => {
      const row = document.createElement("tr");
      const extraction = source.epub_extractions;
      const values = [
        source.source_id,
        timestamp(source.last_crawl_started_at),
        crawlRates[sourceIndex].pages === null ? "Measuring…" : `${rate(crawlRates[sourceIndex].pages)} pages/min · ${rate(crawlRates[sourceIndex].publications)} entries/min`,
        source.crawl_frontier_count == null ? "—" : number(source.crawl_frontier_count),
        number(source.active_page_count),
        number(source.active_publication_count),
        number(extraction.pending + extraction.running),
      ];
      for (const [index, value] of values.entries()) {
        const cell = document.createElement("td");
        if (index === 0) { const code = document.createElement("code"); code.textContent = value; cell.append(code); } else cell.textContent = value;
        row.append(cell);
      }
      return row;
    });
    elements["catalogue-current-crawl"].replaceChildren(...currentCrawlRows);
    const sourceRows = catalogue.sources.map((source) => {
      const row = document.createElement("tr");
      const lastSuccess = source.last_crawl_succeeded_at ? timestamp(source.last_crawl_succeeded_at) : "Never";
      const extraction = source.epub_extractions;
      const crawlStatus = source.last_crawl_error_category ? `${source.crawl_status} · ${source.last_crawl_error_category.replaceAll("_", " ")}` : source.crawl_status;
      for (const [index, value] of [source.source_id, crawlStatus, source.crawl_frontier_count == null ? "—" : number(source.crawl_frontier_count), lastSuccess, number(source.active_publication_count), number(source.active_epub_count), `${number(extraction.pending + extraction.running)} / ${number(extraction.failed)}`].entries()) {
        const cell = document.createElement("td");
        if (index === 0) { const code = document.createElement("code"); code.textContent = value; cell.append(code); } else cell.textContent = value;
        row.append(cell);
      }
      return row;
    });
    elements["catalogue-sources"].replaceChildren(...sourceRows);
    const failures = catalogue.extraction_failures ?? [];
    elements["catalogue-failures-empty"].hidden = failures.length !== 0;
    elements["catalogue-failures-wrap"].hidden = failures.length === 0;
    const failureRows = failures.map((failure) => {
      const row = document.createElement("tr");
      const retry = failure.retryable ? "Retryable with backoff" : "Permanent";
      const observed = `${timestamp(failure.first_failed_at)} / ${timestamp(failure.last_failed_at)}`;
      const examples = failure.examples.map((example) => `${example.source_id} · ${example.acquisition_host} · ${example.publication_id || "unknown"} · ${number(example.attempts)} attempts`).join("; ");
      for (const [index, value] of [failure.category.replaceAll("_", " "), retry, number(failure.count), observed, failure.next_retry_at ? timestamp(failure.next_retry_at) : "—", examples || "—"].entries()) {
        const cell = document.createElement("td");
        if (index === 0) { const code = document.createElement("code"); code.textContent = value; cell.append(code); } else cell.textContent = value;
        row.append(cell);
      }
      return row;
    });
    elements["catalogue-failures"].replaceChildren(...failureRows);
  }

  const metadata = services.metadata.data;
  elements["metadata-unavailable"].hidden = Boolean(metadata);
  elements["metadata-content"].hidden = !metadata;
  if (metadata) {
    elements["metadata-cards"].replaceChildren(
      card("Snapshot", metadata.dump_date, `Schema v${number(metadata.schema_version)} · imported ${new Date(metadata.imported_at_ms).toLocaleDateString()}`),
      card("Snapshot size", bytes(metadata.database_bytes), `${number(metadata.edition_records)} retained editions`),
      card("Retained works", number(metadata.work_records), `${number(metadata.author_records)} retained authors`),
      card("Enrichment batches", number(metadata.requests), `${number(metadata.errors)} errors · ${metadata.average_duration_ms.toFixed(1)} ms/batch`),
      ...(Number.isFinite(metadata.processed_items) ? [card("Enrichment throughput", `${rate(metadata.items_per_processing_second)} items/s`, `${number(metadata.processed_items)} items · ${metadata.average_duration_ms_per_item.toFixed(2)} ms/item`)] : []),
    );
    const metricRows = Object.entries(metadata.metrics).map(([name, value]) => {
      const row = document.createElement("tr");
      const key = document.createElement("td"); const code = document.createElement("code"); code.textContent = name; key.append(code);
      const metricValue = document.createElement("td"); metricValue.textContent = number(value); row.append(key, metricValue); return row;
    });
    elements["metadata-metrics"].replaceChildren(...metricRows);
  }
  const requestRows = requestHistory.points.map((request) => {
    const row = document.createElement("tr");
    const values = [
      new Date(request.occurred_at_ms).toLocaleString(),
      request.client_ip,
      request.route_group,
      request.method,
      `${request.status_class}xx`,
      bytes(request.request_bytes + request.response_bytes),
      `${number(request.duration_ms)} ms`,
    ];
    for (const [index, value] of values.entries()) {
      const cell = document.createElement("td");
      if (index === 1 || index === 2) { const code = document.createElement("code"); code.textContent = value; cell.append(code); } else cell.textContent = value;
      row.append(cell);
    }
    return row;
  });
  elements["request-ips"].replaceChildren(...requestRows);
}

function showError(message, signIn) {
  elements.loading.hidden = true;
  elements.dashboard.hidden = true;
  elements.notice.hidden = false;
  elements.logout.hidden = true;
  elements["notice-message"].textContent = message;
  elements["login-form"].hidden = !signIn;
  elements.retry.hidden = signIn;
}
async function load() {
  if (refreshing) return;
  refreshing = true;
  elements.refresh.disabled = true;
  try {
    const responses = await Promise.all([
      request("/api/admin/overview"),
      request(`/api/admin/traffic?hours=${hours}`),
      request(`/api/admin/requests?hours=${Math.min(hours, 168)}&limit=200`),
      request("/api/admin/engagement?days=30"),
      request("/api/admin/services"),
    ]);
    if (responses.some((response) => response.status === 401)) return showError("Sign in with an administrator account.", true);
    if (responses.some((response) => response.status === 403)) return showError("This account is not an administrator.", true);
    if (responses.some((response) => response.status === 429)) return showError("The dashboard is being refreshed too frequently. Wait a moment and try again.", false);
    if (responses.some((response) => !response.ok)) return showError("The operational API did not respond successfully.", false);
    render(...await Promise.all(responses.map((response) => response.json())));
    elements.loading.hidden = true;
    elements.notice.hidden = true;
    elements.dashboard.hidden = false;
    elements.logout.hidden = false;
    elements.updated.textContent = `Updated ${new Date().toLocaleTimeString()}`;
  } catch {
    showError("The dashboard could not reach the operational API.", false);
  } finally {
    refreshing = false;
    elements.refresh.disabled = false;
  }
}

elements.refresh.addEventListener("click", load);
elements.retry.addEventListener("click", load);
elements.logout.addEventListener("click", async () => {
  elements.logout.disabled = true;
  try { await fetch("/api/auth/web/logout", { method:"POST", credentials:"same-origin", headers:{ Accept:wireMediaType } }); } finally {
    elements.logout.disabled = false;
    elements.updated.textContent = "";
    showError("Signed out. Sign in with an administrator account.", true);
  }
});
elements.ranges.addEventListener("click", (event) => {
  const button = event.target.closest("button[data-hours]");
  if (!button) return;
  hours = Number(button.dataset.hours);
  for (const range of elements.ranges.querySelectorAll("button")) range.classList.toggle("active", range === button);
  load();
});
elements["login-form"].addEventListener("submit", async (event) => {
  event.preventDefault();
  elements.login.disabled = true;
  try {
    const response = await fetch("/api/admin/login", { method:"POST", credentials:"same-origin", headers:{ "Content-Type":"application/json", Accept:"application/json" }, body:JSON.stringify({ email:elements.email.value, password:elements.password.value }) });
    elements.password.value = "";
    if (!response.ok) {
      const message = response.status === 429
        ? "Too many sign-in attempts. Wait before trying again."
        : response.status === 403
          ? "Firefox did not permit the same-origin sign-in request. Reload the page and try again."
          : "Administrator email or password is incorrect.";
      return showError(message, true);
    }
    await load();
  } catch {
    showError("The sign-in request could not reach the server.", true);
  } finally {
    elements.login.disabled = false;
  }
});

selectPage();
load();
window.setInterval(() => { if (!elements.dashboard.hidden) load(); }, 15_000);
