import {
  createRouter,
  createRoute,
  createRootRoute,
  Outlet,
  Navigate,
} from "@tanstack/react-router";
import App from "./App";
import AlgorithmPage from "./pages/AlgorithmPage";

const rootRoute = createRootRoute({
  component: () => <Outlet />,
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
