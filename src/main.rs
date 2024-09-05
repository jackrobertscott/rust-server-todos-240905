use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, Validate)]
struct Todo {
    #[validate(length(min = 1, max = 100))]
    title: String,
    completed: bool,
}

type TodoList = Arc<Mutex<Vec<Todo>>>;

async fn todo_handler(
    req: Request<hyper::body::Incoming>,
    todos: TodoList,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&Method::POST, "/todos") => create_todo(req, todos).await,
        (&Method::GET, "/todos") => list_todos(todos),
        (&Method::PUT, "/todos") => update_todo(req, todos).await,
        (&Method::DELETE, "/todos") => delete_todo(req, todos).await,
        _ => Ok(error_response(
            StatusCode::NOT_FOUND,
            "Not Found".to_string(),
        )),
    }
}

async fn create_todo(
    req: Request<hyper::body::Incoming>,
    todos: TodoList,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let todo: Todo = parse_json(req).await?;
    if let Err(errors) = todo.validate() {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            format!("Validation error: {:?}", errors),
        ));
    }
    todos.lock().unwrap().push(todo);
    json_response(&todos.lock().unwrap().last().unwrap())
}

fn list_todos(todos: TodoList) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    json_response(&*todos.lock().unwrap())
}

async fn update_todo(
    req: Request<hyper::body::Incoming>,
    todos: TodoList,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let todo: Todo = parse_json(req).await?;
    if let Err(errors) = todo.validate() {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            format!("Validation error: {:?}", errors),
        ));
    }
    let mut todos = todos.lock().unwrap();
    if let Some(existing_todo) = todos.iter_mut().find(|t| t.title == todo.title) {
        *existing_todo = todo;
        json_response(existing_todo)
    } else {
        Ok(error_response(
            StatusCode::NOT_FOUND,
            "Todo not found".to_string(),
        ))
    }
}

async fn delete_todo(
    req: Request<hyper::body::Incoming>,
    todos: TodoList,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let todo: Todo = parse_json(req).await?;
    let mut todos = todos.lock().unwrap();
    if let Some(index) = todos.iter().position(|t| t.title == todo.title) {
        todos.remove(index);
        Ok(Response::new(empty()))
    } else {
        Ok(error_response(
            StatusCode::NOT_FOUND,
            "Todo not found".to_string(),
        ))
    }
}

async fn parse_json<T: serde::de::DeserializeOwned>(
    req: Request<hyper::body::Incoming>,
) -> Result<T, hyper::Error> {
    let body_bytes = req.collect().await?.to_bytes();
    Ok(serde_json::from_slice(&body_bytes).unwrap())
}

fn json_response<T: serde::Serialize>(
    data: &T,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let json = serde_json::to_string(data).unwrap();
    let mut response = Response::new(full(json));
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    Ok(response)
}

fn error_response(status: StatusCode, message: String) -> Response<BoxBody<Bytes, hyper::Error>> {
    let mut response = Response::new(full(message));
    *response.status_mut() = status;
    response
}

fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3100));
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);

    let todos: TodoList = Arc::new(Mutex::new(Vec::new()));

    loop {
        let (stream, _) = listener.accept().await?;
        let todos = todos.clone();
        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .serve_connection(
                    TokioIo::new(stream),
                    service_fn(|req| todo_handler(req, todos.clone())),
                )
                .await
            {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}
