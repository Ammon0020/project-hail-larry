/*
 * Minimal offline SPA shell cache for the Local Agent Interface frontend.
 *
 * Goal: a page reload while the LAN daemon is unreachable (or the device is
 * offline) still renders the last shell — at minimum the login/pair UI — so
 * the user is not staring at a browser "no internet" page. Live data (/api,
 * /ws) always needs the network and is intentionally NOT cached.
 *
 * Strategy:
 *   - Precache the stable shell files on install (index.html + favicons).
 *   - Navigations (HTML documents): network-first; on failure fall back to the
 *     cached index.html so the SPA boots and routing takes over.
 *   - Hashed bundle assets under /assets/*: stale-while-revalidate. They are
 *     fetched and cached on the first online visit, then served from cache on
 *     the next (possibly offline) reload. Content hashing makes this safe.
 *   - /api/* and /ws*: never intercepted (NetworkOnly). Credentials, live
 *     state, and agent traffic stay out of the cache entirely.
 *
 * The pairing credential lives in localStorage (not the Cache API) and is
 * untouched by this worker — it survives reloads independently.
 *
 * This is deliberately small and dependency-free (no workbox). Bump
 * SHELL_CACHE_VERSION to invalidate the whole cache on a new release.
 */

// Bump to force a clean cache swap on the next deploy.
const SHELL_CACHE_VERSION = 'v1';
const SHELL_CACHE = `lai-shell-${SHELL_CACHE_VERSION}`;
const ASSET_CACHE = `lai-assets-${SHELL_CACHE_VERSION}`;

// Stable shell files (no content hash) precached at install time. The hashed
// JS/CSS bundles are picked up at runtime via stale-while-revalidate instead,
// so this list never needs to track build output filenames.
const PRECACHE_URLS = [
  '/',
  '/index.html',
  '/favicon.svg',
  '/icons.svg',
];

// Routes that must always go to the network and never touch the cache.
// Matches the backend's SPA-fallback exclusions in internal/server/server.go.
function isNetworkOnly(url) {
  const { pathname } = url;
  return pathname.startsWith('/api/') || pathname.startsWith('/ws');
}

self.addEventListener('install', (event) => {
  // skipWaiting() makes a new SW take over immediately on install instead of
  // waiting for all tabs to close — gives an autoUpdate-like UX with no
  // client messaging needed.
  self.skipWaiting();
  event.waitUntil(
    (async () => {
      const cache = await caches.open(SHELL_CACHE);
      // Precache shell files individually so one 404 doesn't abort the rest.
      await Promise.all(
        PRECACHE_URLS.map(async (url) => {
          try {
            await cache.add(new Request(url, { cache: 'reload' }));
          } catch (err) {
            // A missing precache target (e.g. icons.svg not yet built) is
            // non-fatal — runtime caching still covers it later.
            console.warn('[sw] precache miss for', url, err);
          }
        }),
      );
    })(),
  );
});

self.addEventListener('activate', (event) => {
  // claim() lets this SW control the page that triggered the install on the
  // very first visit, so the fetch handler is active for the current reload.
  event.waitUntil(
    (async () => {
      await self.clients.claim();
      // Drop any caches from previous versions.
      const keys = await caches.keys();
      await Promise.all(
        keys
          .filter((k) => k !== SHELL_CACHE && k !== ASSET_CACHE)
          .map((k) => caches.delete(k)),
      );
    })(),
  );
});

self.addEventListener('fetch', (event) => {
  const req = event.request;

  // Only handle same-origin GET. POST/PUT/etc. and cross-origin (e.g. CDN
  // fonts if any) are passed straight to the browser.
  const url = new URL(req.url);
  if (req.method !== 'GET' || url.origin !== self.location.origin) {
    return;
  }

  // Never cache live data or auth-bearing endpoints. Letting these fall
  // through means the network handles them (and they fail normally offline,
  // which is the expected behavior — live data needs the daemon).
  if (isNetworkOnly(url)) {
    return;
  }

  // Navigations (HTML page loads): network-first with cached-shell fallback.
  if (req.mode === 'navigate') {
    event.respondWith(networkFirstNavigation(req));
    return;
  }

  // Static assets (hashed bundles under /assets/*, favicons, etc.):
  // stale-while-revalidate. Serve cache instantly when present, refresh in the
  // background so the next load is fresh. Content-hashed filenames make
  // serving stale bytes safe.
  event.respondWith(staleWhileRevalidate(req));
});

/**
 * Network-first handler for navigation requests.
 *
 * Tries the network so an online reload always reflects the latest shell. On
 * any network failure (daemon down, device offline, DNS error) it falls back
 * to the cached index.html so the SPA still boots and client-side routing can
 * render the last-known shell (login/pair UI).
 *
 * Args:
 *   req: The navigation Request.
 *
 * Returns:
 *   A Response — either fresh from the network or the cached shell.
 */
async function networkFirstNavigation(req) {
  try {
    const fresh = await fetch(req);
    // Cache the latest shell so future offline reloads get this version.
    const cache = await caches.open(SHELL_CACHE);
    cache.put('/index.html', fresh.clone()).catch(() => {
      /* best-effort; don't block the response */
    });
    return fresh;
  } catch (err) {
    const cached =
      (await caches.match('/index.html')) || (await caches.match('/'));
    if (cached) {
      return cached;
    }
    // No shell cached yet — surface a minimal offline notice instead of a
    // raw browser error so the user understands the daemon is unreachable.
    return new Response(
      '<!doctype html><meta charset="utf-8"><title>Offline</title>' +
        '<body style="font:14px system-ui;padding:2rem;max-width:34rem">' +
        '<h1>Offline</h1><p>The Local Agent Interface daemon is unreachable ' +
        'and no cached shell is available yet. Reconnect to the LAN and ' +
        'reload.</p></body>',
      {
        status: 503,
        headers: { 'Content-Type': 'text/html; charset=utf-8' },
      },
    );
  }
}

/**
 * Stale-while-revalidate handler for same-origin static assets.
 *
 * Returns the cached response immediately if present and refreshes the cache
 * in the background. If nothing is cached, goes to the network and caches the
 * result. Falls back to cache on network failure.
 *
 * Args:
 *   req: The asset Request.
 *
 * Returns:
 *   A Response from cache or network.
 */
async function staleWhileRevalidate(req) {
  const cache = await caches.open(ASSET_CACHE);
  const cached = await cache.match(req);

  const networkPromise = fetch(req)
    .then((fresh) => {
      // Only cache successful, basic (CORS-same-origin) responses.
      if (fresh && fresh.ok && fresh.type === 'basic') {
        cache.put(req, fresh.clone()).catch(() => {
          /* best-effort */
        });
      }
      return fresh;
    })
    .catch(() => cached);

  // Serve stale immediately if we have it; otherwise wait for the network.
  return cached || networkPromise;
}
