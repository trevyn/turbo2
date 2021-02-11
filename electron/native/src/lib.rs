use neon::{event::EventHandler, prelude::*};
use once_cell::sync::Lazy;
use std::{collections::HashMap, future::Future, pin::Pin, sync::Mutex, task::Poll};

#[allow(clippy::type_complexity)]
static PROSEMIRROR_CALLBACK: Lazy<
 Mutex<Option<Box<dyn Fn(String, String) -> Pin<Box<dyn Future<Output = String>>> + Send>>>,
> = Lazy::new(|| Mutex::new(None));

static RESULTWAKERS: Lazy<Mutex<HashMap<u8, ResultWaker>>> =
 Lazy::new(|| Mutex::new(HashMap::new()));

#[allow(clippy::unnecessary_wraps)]
fn hello(mut cx: FunctionContext) -> JsResult<JsString> {
 if let Some(c) = PROSEMIRROR_CALLBACK.lock().unwrap().as_ref() {
  eprintln!("calling c()");
  c("10".to_string(), "20".to_string());
 } else {
  eprintln!("c not ready yet!");
 };

 Ok(cx.string("hello node 2"))
}

async fn do_thing() {
 eprintln!("do thing");
 my_fut().await;
}

fn my_fut() -> Pin<Box<dyn Future<Output = String>>> {
 Box::pin(MyFut { slot: 0 })
}

fn helper(mut cx: FunctionContext) -> JsResult<JsUndefined> {
 let slot = cx.argument::<JsNumber>(0)?.value() as u8;
 let result = cx.argument::<JsString>(1)?.value();

 let mut resultwakers = RESULTWAKERS.lock().unwrap();
 let resultwaker = resultwakers.get_mut(&slot).unwrap();

 resultwaker.result = Some(result);
 if let Some(waker) = resultwaker.waker.take() {
  waker.wake();
 }

 Ok(cx.undefined())
}

#[derive(Debug)]
struct ResultWaker {
 result: Option<String>,
 waker: Option<std::task::Waker>,
}

struct MyFut {
 slot: u8,
}

impl Future for MyFut {
 type Output = String;

 fn poll(self: Pin<&mut Self>, cx: &mut core::task::Context) -> Poll<Self::Output> {
  let slot = &self.slot;
  let mut resultwakers = RESULTWAKERS.lock().unwrap();
  let resultwaker = resultwakers.get_mut(slot).unwrap();

  match resultwaker.result.take() {
   None => {
    resultwaker.waker = Some(cx.waker().clone());
    Poll::Pending
   }
   Some(result) => {
    resultwakers.remove(slot);
    Poll::Ready(result)
   }
  }
 }
}

fn register_prosemirror_apply_callback(mut cx: FunctionContext) -> JsResult<JsUndefined> {
 let this = cx.this();
 let func = cx.argument::<JsFunction>(0)?;
 let eventhandler = EventHandler::new(&cx, this, func);

 let execute = move |old: String, patch: String| -> Pin<Box<dyn Future<Output = String>>> {
  let slot = {
   let mut resultwakers = RESULTWAKERS.lock().unwrap();
   let slot = (1..200).find(|n| !resultwakers.contains_key(n)).unwrap();
   assert!(resultwakers.insert(slot, ResultWaker { result: None, waker: None }).is_none());
   slot
  };

  let fut = MyFut { slot };

  eventhandler.schedule(move |cx: &mut neon::context::TaskContext| {
   eprintln!("in scheduled");

   let helper = neon::types::JsFunction::new(cx, helper).unwrap();

   let args: Vec<Handle<JsValue>> = vec![
    helper.upcast(),
    cx.number(slot).upcast(),
    cx.string(old).upcast(),
    cx.string(patch).upcast(),
   ];

   args
  });

  Box::pin(fut)
 };

 *PROSEMIRROR_CALLBACK.lock().unwrap() = Some(Box::new(execute));
 Ok(cx.undefined())
}

fn rust_log(mut cx: FunctionContext) -> JsResult<JsUndefined> {
 let s = cx.argument::<JsString>(0)?.value();
 eprintln!("rust_log: {:#?}", s);
 turbo_server::rust_log(s);
 Ok(cx.undefined())
}

#[neon::main]
fn my_module(mut cx: ModuleContext) -> NeonResult<()> {
 log::debug!("neon module init");
 cx.export_function("hello", hello)?;
 cx.export_function("rustLog", rust_log)?;
 cx.export_function("registerProsemirrorApplyCallback", register_prosemirror_apply_callback)?;

 // launch the server
 std::thread::spawn(|| {
  turbo_server::run();
 });

 Ok(())
}
