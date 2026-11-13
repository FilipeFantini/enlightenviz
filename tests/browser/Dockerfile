# A headless Chromium and Node, to drive the page in a real browser. Used by
# run.sh for the smoke test and for the screenshots. Nothing here is installed
# on the host. The scripts speak the Chrome DevTools Protocol directly (Node's
# built-in WebSocket), so there is no npm package and no package.json.

FROM alpine:3.24@sha256:f59abe656b692d901e184be3445e084786602108599d2a51d229c0f5a48c71bf
# font-noto: system-ui needs some font to resolve to, and the screenshots
# should not depend on which one the base image happens to have.
RUN apk add --no-cache chromium nodejs font-noto
COPY cdp.mjs smoke.mjs capture.mjs /tests/
WORKDIR /tests
