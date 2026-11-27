// Wren's phone page: shows push notifications sent by Wren on the computer
// (encrypted for this browser; the push service can't read them). Nothing is
// cached: the page always comes fresh from the relay.

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));

self.addEventListener('push', (e) => {
  let d = {};
  try {
    d = e.data ? e.data.json() : {};
  } catch {
    d = { body: e.data ? e.data.text() : '' };
  }
  e.waitUntil(
    self.registration.showNotification(d.title || 'Wren', {
      body: d.body || '',
      tag: d.tag || 'wren',
      renotify: true,
      icon: 'icon-192.png',
      badge: 'icon-192.png',
      data: { tag: d.tag || '' },
    }),
  );
});

self.addEventListener('notificationclick', (e) => {
  e.notification.close();
  const slot = String(e.notification.data?.tag || '').replace(/^pane-/, '');
  const url = new URL(slot ? `./#pane=${slot}` : './', self.registration.scope).href;
  e.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((list) => {
      for (const c of list) {
        c.postMessage({ open: slot });
        return c.focus();
      }
      return self.clients.openWindow(url);
    }),
  );
});
