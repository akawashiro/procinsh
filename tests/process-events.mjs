// Capture wire observations without exposing application state on window.
export const processEventRecording = `(() => {
  window.targetSources = [];
  window.processObservations = [];
  window.pageRequests = [];
  const nativeFetch = window.fetch;
  window.fetch = (...args) => {
    window.pageRequests.push(String(args[0]));
    return nativeFetch(...args);
  };
  const Native = window.EventSource;
  window.EventSource = class extends Native {
    constructor(...args) {
      super(...args);
      if (!String(args[0]).startsWith('/api/processes/events?')) return;
      window.targetSources.push(this);
      this.addEventListener('observation', event => {
        try {
          const data = JSON.parse(event.data);
          window.processObservations.push(data);
          window.lastObservation = data;
        } catch {}
      });
      queueMicrotask(() => this.dispatchEvent(new Event('error')));
    }
  };
})();`;
