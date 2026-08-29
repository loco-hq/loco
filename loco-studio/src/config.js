// API origin the Studio SPA talks to.
//
// Empty string = this page's own origin. That is the only value that works
// once Studio is a bundle served from a site URL (`docs/hosting.md`) — the
// page cannot know in advance which host it was deployed under. In dev it is
// the Vite proxy on :5174, which forwards /auth /config /schema /data to
// :3000, so the inner loop is unchanged.
//
// Point it at an absolute origin only for a build you deliberately serve
// from somewhere else. CORS is `*`, so that is legal — it is just not how a
// bundle on a site is reached.
export const API_ORIGIN = '';
