import { Suspense } from "react";
import { Route, Router, Switch } from "wouter";
import { useHashLocation } from "wouter/use-hash-location";
import { Gallery } from "@minutae/ui";
import Shell from "./shell/Shell";
import { paths, screens as S } from "./routes";

export default function App() {
  return (
    <Router hook={useHashLocation}>
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
