import { Suspense } from "react";
import { Route, Router, Switch } from "wouter";
import { Gallery } from "@minutae/ui";
import { useAppearance, usePlatformInfo } from "./hooks";
import { useHashPath, useHashSearch } from "./lib/hashLocation";
import Shell from "./shell/Shell";
import { paths, screens as S } from "./routes";

export default function App() {
  // Every window (main, HUD, call prompt) follows Settings → General → Appearance.
  useAppearance();
  const platform = usePlatformInfo();
  return (
    <Router hook={useHashPath} searchHook={useHashSearch}>
      {/* Test hook: copy depends on the platform answer; screens add their own data-ready. */}
      <span hidden data-ready={String(platform !== null)} />
      <Suspense fallback={null}>
        <Switch>
          {/* Overlay windows: no shell */}
          <Route path={paths.hud} component={S.hud} />
          <Route path={paths.prompt} component={S.prompt} />
          <Route path={paths.onboarding} component={S.onboarding} />
          <Route path={paths.gallery}>{() => <Gallery />}</Route>
          <Route>
            <Shell>
              <Suspense fallback={null}>
                <Switch>
                  <Route path={paths.meetings} component={S.meetings} />
                  <Route path={paths.detail} component={S.detail} />
                  <Route path={paths.live} component={S.live} />
                  <Route path={paths.dictate} component={S.dictate} />
                  <Route path={paths.library} component={S.library} />
                  <Route path={paths.settings} component={S.settings} />
                </Switch>
              </Suspense>
            </Shell>
          </Route>
        </Switch>
      </Suspense>
    </Router>
  );
}
