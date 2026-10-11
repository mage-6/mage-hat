// The browser side of web analytics (the analytics skill, clients/web): one small function
// that posts contract batches, to the site's own Worker (`/api/track`) or straight to the
// account's collector for a static site. Inlined into the site's script by the site
// generator; nothing per site but the endpoint and the declared events.
//
// Every event carries the page path and the referrer host as its first two text params,
// never a full URL: query strings carry emails, tokens and campaign ids. Page views come from
// here, not from the server, because crawlers mostly do not run JavaScript, which filters
// them for free. Nothing is stored in the browser; an experiment arm, when one runs, is the
// one value kept, under `analytics.variant`, a label with two to four values.
//
//   const analytics = createBeacon("/api/track");
//   analytics("page_view");
//   analytics("conversion", ["signup"]);
export function createBeacon(endpoint, version = "") {
  let variant = "";
  try {
    variant = localStorage.getItem("analytics.variant") ?? "";
  } catch {
    /* storage blocked: no experiment for this visitor */
  }
  return function beacon(event, text = [], nums = []) {
    let referrer = "";
    try {
      referrer = document.referrer ? new URL(document.referrer).host : "";
    } catch {
      /* an unparsable referrer is no referrer */
    }
    const body = JSON.stringify({
      v: version,
      platform: "web",
      variant,
      events: [{ e: event, t: [location.pathname, referrer, ...text], n: nums }],
    });
    // Never awaited in a navigation path; keepalive lets the request outlive the page.
    if (!navigator.sendBeacon?.(endpoint, body)) {
      fetch(endpoint, { method: "POST", body, keepalive: true }).catch(() => {});
    }
  };
}
