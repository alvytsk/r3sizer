/* eslint-disable react-refresh/only-export-components -- router config module, not HMR-eligible */
import { Suspense, lazy } from "react";
import {
  createRouter,
  createRoute,
  createRootRoute,
  Outlet,
  Navigate,
} from "@tanstack/react-router";
import App from "@/pages/studio/ui/studio-page";

/**
 * AlgorithmPage is lazy-loaded so its KaTeX dependency (JS + ~1 MB of fonts)
 * lands in a separate chunk fetched only when the user opens /algorithm.
 */
const AlgorithmPage = lazy(() => import("@/pages/algorithm/ui/algorithm-page"));

const rootRoute = createRootRoute({
  component: () => (
    <Suspense fallback={null}>
      <Outlet />
    </Suspense>
  ),
  notFoundComponent: () => <Navigate to="/" />,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: App,
});

const algorithmRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/algorithm",
  component: AlgorithmPage,
});

const routeTree = rootRoute.addChildren([indexRoute, algorithmRoute]);

export const router = createRouter({
  routeTree,
  basepath: "/r3sizer",
  defaultNotFoundComponent: () => <Navigate to="/" />,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
