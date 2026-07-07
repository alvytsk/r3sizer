import {
  createRootRoute,
  createRoute,
  createRouter,
  Navigate,
  Outlet,
} from "@tanstack/react-router";
import { lazy, Suspense } from "react";
import StudioPage from "@/pages/studio";

/**
 * AlgorithmPage is lazy-loaded so its KaTeX dependency (JS + ~1 MB of fonts)
 * lands in a separate chunk fetched only when the user opens /algorithm.
 */
const AlgorithmPage = lazy(() => import("@/pages/algorithm"));

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
  component: StudioPage,
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
