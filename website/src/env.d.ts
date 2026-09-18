/// <reference types="astro/client" />
// Starlight's ambient types (App.Locals.t / starlightRoute, StarlightApp) ship
// inside the package entry, not at the pre-0.42 root paths. Importing the
// package pulls them into every file that compiles against this project.
import "@astrojs/starlight";
