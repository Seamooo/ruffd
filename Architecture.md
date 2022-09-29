# Architecture

This document provides a brief overview of the rationale behind the
architecture of this project

## Scheduling

As this is a language server, queries must occur on state as it exists at
the request's point in time. The solution for this has been to represent state
as a number of read / write locks that must be acquired prior to execution.
This allows concurrency where multiple requests are able to acquire their necessary
locks, whilst preserving order of execution such that a request / notification is
operating on state at its point in time, and side effects occur in-order.
As this pattern is fairly laborious, it has been made easier to repeat with the
use of the `#[notification]` and `#[request]` macros. Each taking an optional
argument list of optionally mutable struct members that correspond to struct
members of `ruffd_types::ServerState`.

A critical issue with this pattern is the ease at which deadlock can occur, as multiple
locks are required before execution, it is possible that something else looks to
acquire the locks prior to execution, ending up with a possible contention on
both paths having their lock requests partially fulfilled. This is avoided by isolating
lock acquisition to the handle_loop of the server, where sequential lock acquisition
is enforced. This applies to all work done with server state.

