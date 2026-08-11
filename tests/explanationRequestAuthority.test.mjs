import assert from "node:assert/strict";
import test from "node:test";
import {
  ExplanationRequestAuthority,
  isExplanationRequestCancelled,
} from "../src/explanationRequestAuthority.js";

test("独立 authority 实例不会复用首个同作用域 requestKey", async () => {
  const authorityA = new ExplanationRequestAuthority(() => undefined);
  const authorityB = new ExplanationRequestAuthority(() => undefined);

  const requestA = authorityA.begin("manual");
  const requestB = authorityB.begin("manual");

  assert.notEqual(requestA, requestB);
  assert.match(requestA, /^manual:[A-Za-z0-9_-]+:1$/);
  assert.match(requestB, /^manual:[A-Za-z0-9_-]+:1$/);
});

test("旧实例迟到 invalidate 不会命中新实例 active key", async () => {
  const oldCancellations = [];
  const oldAuthority = new ExplanationRequestAuthority(
    (scope, requestKey) => oldCancellations.push([scope, requestKey]),
    () => "old-session",
  );
  const newAuthority = new ExplanationRequestAuthority(
    () => undefined,
    () => "new-session",
  );

  const oldRequest = oldAuthority.begin("manual");
  const newRequest = newAuthority.begin("manual");
  oldAuthority.invalidate("manual");

  assert.deepEqual(oldCancellations, [["manual", oldRequest]]);
  assert.notEqual(oldRequest, newRequest);
  assert.equal(newAuthority.isCurrent("manual", newRequest), true);
});

test("注入的 session nonce 必须满足 Rust requestKey 边界", async () => {
  const maximumAuthority = new ExplanationRequestAuthority(
    () => undefined,
    () => "a".repeat(96),
  );
  const maximumKey = maximumAuthority.begin("anchored");
  assert.ok(maximumKey.length <= 128);
  assert.match(maximumKey, /^[A-Za-z0-9_:-]+$/);

  assert.throws(
    () => new ExplanationRequestAuthority(() => undefined, () => ""),
    /session nonce 无效/,
  );
  assert.throws(
    () => new ExplanationRequestAuthority(() => undefined, () => "bad:value"),
    /session nonce 无效/,
  );
  assert.throws(
    () =>
      new ExplanationRequestAuthority(() => undefined, () => "a".repeat(97)),
    /session nonce 无效/,
  );
});

test("A→B 会取消旧请求并拒绝底层迟到结果", async () => {
  const cancellations = [];
  const authority = new ExplanationRequestAuthority((scope, requestKey) => {
    cancellations.push([scope, requestKey]);
  });

  const requestA = authority.begin("anchored");
  const requestB = authority.begin("anchored");

  assert.deepEqual(cancellations, [["anchored", requestA]]);
  assert.equal(authority.isCurrent("anchored", requestA), false);
  assert.equal(authority.finish("anchored", requestA), false);
  assert.equal(authority.isCurrent("anchored", requestB), true);
});

test("manual 与 anchored 权威互不误取消", async () => {
  const cancellations = [];
  const authority = new ExplanationRequestAuthority((scope, requestKey) => {
    cancellations.push([scope, requestKey]);
  });

  const manual = authority.begin("manual");
  const anchored = authority.begin("anchored");
  authority.invalidate("manual");

  assert.deepEqual(cancellations, [["manual", manual]]);
  assert.equal(authority.isCurrent("anchored", anchored), true);
});

test("编辑、隐藏、模式切换和卸载都可使当前请求失效", async () => {
  const cancellations = [];
  const authority = new ExplanationRequestAuthority((scope, requestKey) => {
    cancellations.push([scope, requestKey]);
  });

  const edited = authority.begin("manual");
  authority.invalidate("manual");
  const hiddenManual = authority.begin("manual");
  const hiddenAnchored = authority.begin("anchored");
  authority.invalidateAll();
  const switched = authority.begin("anchored");
  authority.invalidateAll();

  assert.deepEqual(cancellations, [
    ["manual", edited],
    ["manual", hiddenManual],
    ["anchored", hiddenAnchored],
    ["anchored", switched],
  ]);
});

test("失败完成后可使用新 key 重试，旧 key 不会恢复权威", async () => {
  const authority = new ExplanationRequestAuthority(() => undefined);

  const failed = authority.begin("manual");
  assert.equal(authority.finish("manual", failed), true);
  const retry = authority.begin("manual");

  assert.notEqual(retry, failed);
  assert.equal(authority.isCurrent("manual", failed), false);
  assert.equal(authority.isCurrent("manual", retry), true);
});

test("取消错误使用稳定标记且不会冒充网络或 schema 错误", async () => {
  assert.equal(
    isExplanationRequestCancelled("READRAY_EXPLANATION_REQUEST_CANCELLED"),
    true,
  );
  assert.equal(isExplanationRequestCancelled("ExplanationCard schema 错误"), false);
  assert.equal(isExplanationRequestCancelled("网络错误"), false);
});
