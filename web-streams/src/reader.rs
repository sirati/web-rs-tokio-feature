use std::future::Future;
use std::marker::PhantomData;

use js_sys::Reflect;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{js_sys, ReadableStream, ReadableStreamDefaultReader, ReadableStreamReadResult};

use crate::{Error, PromiseExt};

/// A wrapper around ReadableStream
pub struct Reader<T: JsCast> {
	inner: ReadableStreamDefaultReader,

	// Keep the most recent promise to make `read` cancelable
	read: Option<js_sys::Promise>,

	_phantom: PhantomData<T>,
}

impl<T: JsCast> Reader<T> {
	/// Grab a lock on the given readable stream until dropped.
	pub fn new(stream: &ReadableStream) -> Result<Self, Error> {
		let inner = stream.get_reader().unchecked_into();
		Ok(Self {
			inner,
			read: None,
			_phantom: PhantomData,
		})
	}

	/// Read the next element from the stream, returning None if the stream is done.
	pub async fn read(&mut self) -> Result<Option<T>, Error> {
		if self.read.is_none() {
			self.read = Some(self.inner.read());
		}

		let result: ReadableStreamReadResult = JsFuture::from(self.read.as_ref().unwrap().clone()).await?.into();
		self.read.take(); // Clear the promise on success

		if Reflect::get(&result, &"done".into())?.is_truthy() {
			return Ok(None);
		}

		let res = Reflect::get(&result, &"value".into())?.unchecked_into();

		Ok(Some(res))
	}

	/// Abort the stream early with the given reason.
	pub fn abort(&mut self, reason: &str) {
		let str = JsValue::from_str(reason);
		self.inner.cancel_with_reason(&str).ignore();
	}

	pub async fn closed(&self) -> Result<(), Error> {
		JsFuture::from(self.inner.closed()).await?;
		Ok(())
	}
}

impl<T: JsCast> Drop for Reader<T> {
	/// Release the lock
	fn drop(&mut self) {
		self.inner.release_lock();
	}
}


use std::pin::Pin;
use std::task::{Context, Poll};
use wasm_bindgen::JsCast;
use js_sys::Uint8Array;

#[cfg(feature = "tokio")]
impl tokio::io::AsyncRead for Reader<Uint8Array> {

	fn poll_read(
		mut self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &mut tokio::io::ReadBuf<'_>,
	) -> Poll<std::io::Result<()>> {
		if self.read.is_none() {
			self.read = Some(self.inner.read());
		}

		let promise = self.read.as_ref().unwrap();
		let mut js_fut = JsFuture::from(promise.clone());
		match Pin::new(&mut js_fut).poll(cx) {
			Poll::Pending => Poll::Pending,
			Poll::Ready(Ok(js_val)) => {
				self.read.take();
				let result: ReadableStreamReadResult = js_val.unchecked_into();
				if js_sys::Reflect::get(&result, &"done".into()).unwrap().as_bool().unwrap_or(false) {
					return Poll::Ready(Ok(())); // EOF
				}
				let value = js_sys::Reflect::get(&result, &"value".into()).unwrap();
				let array: Uint8Array = value.unchecked_into();
				let array_len = array.length() as usize;
				let len = std::cmp::min(buf.remaining(), array_len);

				// Copy what fits
				array.slice(0, len as u32).copy_to_uninit(unsafe {buf.unfilled_mut()});
				unsafe { buf.assume_init(len); }
				buf.advance(len);

				// If there are leftover bytes, create a new ReadableStreamReadResult and set self.read
				if len < array_len {
					let leftover = array.slice(len as u32, array_len as u32);
					let obj = js_sys::Object::new();
					js_sys::Reflect::set(&obj, &"done".into(), &JsValue::FALSE).unwrap();
					js_sys::Reflect::set(&obj, &"value".into(), &leftover).unwrap();
					let promise = js_sys::Promise::resolve(&obj);
					self.read = Some(promise);
				}

				Poll::Ready(Ok(()))
			}
			Poll::Ready(Err(_)) => {
				self.read.take();
				Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, "js read error")))
			}
		}
	}
}