(() => {
  // frappe/public/js/lib/posthog.js
  !function(t, e) {
    var o, n, p, r;
    e.__SV || (window.posthog = e, e._i = [], e.init = function(i, s, a) {
      function g(t2, e2) {
        var o2 = e2.split(".");
        2 == o2.length && (t2 = t2[o2[0]], e2 = o2[1]), t2[e2] = function() {
          t2.push([e2].concat(Array.prototype.slice.call(arguments, 0)));
        };
      }
      (p = t.createElement("script")).type = "text/javascript", p.async = true, p.src = s.api_host + "/static/array.js", (r = t.getElementsByTagName("script")[0]).parentNode.insertBefore(p, r);
      var u = e;
      for (void 0 !== a ? u = e[a] = [] : a = "posthog", u.people = u.people || [], u.toString = function(t2) {
        var e2 = "posthog";
        return "posthog" !== a && (e2 += "." + a), t2 || (e2 += " (stub)"), e2;
      }, u.people.toString = function() {
        return u.toString(1) + ".people (stub)";
      }, o = "capture identify alias people.set people.set_once set_config register register_once unregister opt_out_capturing has_opted_out_capturing opt_in_capturing reset isFeatureEnabled onFeatureFlags".split(" "), n = 0; n < o.length; n++)
        g(u, o[n]);
      e._i.push([i, s, a]);
    }, e.__SV = 1);
  }(document, window.posthog || []);

  // frappe/public/js/telemetry/posthog.js
  var PosthogProvider = class {
    constructor() {
      this.enabled = false;
      this.project_id = null;
      this.telemetry_host = null;
    }
    is_enabled() {
      var _a;
      return ((_a = frappe.boot.telemetry_provider) == null ? void 0 : _a.includes("posthog")) && frappe.boot.enable_telemetry && Boolean(frappe.boot.posthog_project_id && frappe.boot.posthog_host);
    }
    init() {
      if (!this.is_enabled())
        return;
      this.project_id = frappe.boot.posthog_project_id;
      this.telemetry_host = frappe.boot.posthog_host;
      this.enabled = true;
      try {
        let disable_decide = !this.should_record_session();
        posthog.init(this.project_id, {
          api_host: this.telemetry_host,
          autocapture: false,
          capture_pageview: false,
          capture_pageleave: false,
          advanced_disable_decide: disable_decide
        });
        posthog.identify(frappe.boot.sitename);
        this.send_heartbeat();
        this.register_pageview_handler();
      } catch (e) {
        console.trace("Failed to initialize posthog telemetry", e);
        this.enabled = false;
      }
    }
    capture(event, app, props) {
      if (!this.enabled)
        return;
      posthog.capture(`${app}_${event}`, props);
    }
    send_heartbeat() {
      var _a, _b;
      const KEY = "ph_last_heartbeat";
      const now = frappe.datetime.system_datetime(true);
      const last = localStorage.getItem(KEY);
      if (!last || moment(now).diff(moment(last), "hours") > 12) {
        localStorage.setItem(KEY, now.toISOString());
        this.capture("heartbeat", "frappe", { frappe_version: (_b = (_a = frappe.boot) == null ? void 0 : _a.versions) == null ? void 0 : _b.frappe });
      }
    }
    register_pageview_handler() {
      const site_age = frappe.boot.telemetry_site_age;
      if (site_age && site_age > 6) {
        return;
      }
      frappe.router.on("change", () => {
        posthog.capture("$pageview");
      });
    }
    should_record_session() {
      let start = frappe.boot.sysdefaults.session_recording_start;
      if (!start)
        return;
      let start_datetime = frappe.datetime.str_to_obj(start);
      let now = frappe.datetime.now_datetime();
      return frappe.datetime.get_minute_diff(now, start_datetime) < 120;
    }
  };
  var posthog_provider = new PosthogProvider();

  // frappe/public/js/telemetry/pulse.js
  var PulseProvider = class {
    constructor() {
      this.enabled = false;
      this.eq = null;
    }
    is_enabled() {
      var _a;
      return ((_a = frappe.boot.telemetry_provider) == null ? void 0 : _a.includes("pulse")) && frappe.boot.enable_telemetry;
    }
    init() {
      if (!this.is_enabled())
        return;
      this.enabled = true;
      try {
        this.eq = new QueueManager((events) => this.sendEvents(events), {
          flushInterval: 1e4
        });
        window.addEventListener("beforeunload", () => {
          var _a, _b;
          const events = ((_b = (_a = this.eq) == null ? void 0 : _a.getBufferedEvents) == null ? void 0 : _b.call(_a)) || [];
          if (events.length)
            this.sendBeacon(events);
        });
      } catch (error) {
      }
    }
    capture(event, app, props) {
      var _a;
      if (!this.enabled)
        return;
      this.eq.add({
        event_name: event,
        app,
        properties: props,
        user: (_a = frappe.session) == null ? void 0 : _a.user,
        captured_at: new Date().toISOString()
      });
    }
    sendEvents(events) {
      return new Promise((resolve, reject) => {
        try {
          frappe.call({
            method: "frappe.utils.telemetry.pulse.client.bulk_capture",
            args: { events },
            type: "POST",
            no_spinner: true,
            freeze: false,
            callback: () => resolve(),
            error: (error) => reject(error)
          });
        } catch (error) {
          reject(error);
        }
      });
    }
    sendBeacon(events) {
      try {
        if (navigator.sendBeacon) {
          const url = "/api/method/frappe.utils.telemetry.pulse.client.bulk_capture";
          const data = new FormData();
          data.append("events", JSON.stringify(events));
          navigator.sendBeacon(url, data);
        }
      } catch (error) {
      }
    }
  };
  var QueueManager = class {
    constructor(flushCallback, options = {}) {
      this.flushCallback = flushCallback;
      this.queue = [];
      this.pendingBatch = null;
      this.retryAttempts = 0;
      this.maxRetries = 3;
      this.maxQueueSize = options.maxQueueSize || 20;
      this.flushInterval = options.flushInterval || 5e3;
      this.timer = null;
      this.flushing = false;
      this.start();
    }
    getBufferedEvents() {
      var _a;
      const events = [];
      if ((_a = this.pendingBatch) == null ? void 0 : _a.length)
        events.push(...this.pendingBatch);
      if (this.queue.length)
        events.push(...this.queue);
      return events;
    }
    start() {
      this.timer = setInterval(() => {
        if (this.queue.length || this.pendingBatch)
          this.flush();
      }, this.flushInterval);
    }
    add(event) {
      this.queue.push(event);
      if (this.queue.length >= this.maxQueueSize) {
        this.flush();
      }
    }
    async flush() {
      if (this.flushing)
        return;
      this.flushing = true;
      try {
        if (!this.pendingBatch) {
          if (!this.queue.length)
            return;
          this.pendingBatch = this.queue.splice(0, this.maxQueueSize);
          this.retryAttempts = 0;
        }
        try {
          await this.flushCallback(this.pendingBatch);
          this.pendingBatch = null;
          this.retryAttempts = 0;
        } catch (error) {
          this.retryAttempts++;
          if (this.retryAttempts > this.maxRetries) {
            this.pendingBatch = null;
            this.retryAttempts = 0;
          }
        }
      } finally {
        this.flushing = false;
      }
    }
    stop() {
      if (this.timer) {
        clearInterval(this.timer);
        this.timer = null;
      }
      this.flush();
    }
  };
  var pulse_provider = new PulseProvider();

  // frappe/public/js/telemetry/index.js
  var TelemetryManager = class {
    constructor() {
      var _a, _b;
      this.enabled = frappe.boot.enable_telemetry || false;
      this.posthog_available = Boolean((_a = frappe.boot.telemetry_provider) == null ? void 0 : _a.includes("posthog"));
      this.pulse_available = Boolean((_b = frappe.boot.telemetry_provider) == null ? void 0 : _b.includes("pulse"));
      this.init_providers();
    }
    init_providers() {
      this.providers = [];
      posthog_provider.init();
      if (posthog_provider.enabled) {
        this.providers.push(posthog_provider);
      }
      pulse_provider.init();
      if (pulse_provider.enabled) {
        this.providers.push(pulse_provider);
      }
    }
    capture(event, app, props) {
      if (!this.enabled)
        return;
      for (let provider of this.providers) {
        provider.capture(event, app, props);
      }
    }
    disable() {
      this.enabled = false;
      this.providers = [];
    }
    can_enable() {
      let sentry_available = Boolean(frappe.boot.sentry_dsn);
      return this.posthog_available || this.pulse_available || sentry_available;
    }
  };
  frappe.telemetry = new TelemetryManager();
})();
//# sourceMappingURL=telemetry.bundle.DQEBDMFO.js.map
