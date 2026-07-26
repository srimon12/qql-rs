import init, { compile, isValid, parse } from 'qql-wasm';

async function run() {
    await init();

    const query = "QUERY 'browser search' FROM docs LIMIT 5";
    
    // 1. Validate syntax
    console.log("Is valid QQL:", isValid(query));

    // 2. Parse AST
    const ast = parse(query);
    console.log("Parsed AST:", ast);

    // 3. Client-side compile to Qdrant REST payload
    const payload = compile(query);
    console.log("Qdrant REST Payload:", payload);
}

run();
