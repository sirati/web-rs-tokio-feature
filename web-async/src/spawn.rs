use std::future::Future;

// It's not pretty, but we have per-platform implementations of spawn.
// The main problem is Send; it's an annoying trait that colors everything.
// The rest of this crate is Send agnostic so it will work on WASM.
// TODO: use a send feature and make this runtime agnostic?

#[cfg(any(not(target_arch = "wasm32"), target_os = "wasi"))]
pub fn spawn<F: Future<Output = ()> + Send + 'static>(f: F) {
	#[cfg(feature = "tracing")]
	let f = tracing::Instrument::in_current_span(f);
	tokio::task::spawn(f);
}

#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
pub fn spawn<F: Future<Output = ()> + 'static>(f: F) {
	#[cfg(feature = "tracing")]
	let f = tracing::Instrument::in_current_span(f);
	wasm_bindgen_futures::spawn_local(f);
}
