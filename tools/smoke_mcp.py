"""Smoke-test the Streamable HTTP MCP endpoint, including optional D1 writes."""

from __future__ import annotations

import argparse
import json
import os
import uuid
import urllib.error
import urllib.request
from typing import Any

USER_AGENT = "pricebook-mcp-smoke/0.1"


class McpClient:
    def __init__(self, base_url: str, token: str) -> None:
        self.endpoint = base_url.rstrip("/") + "/mcp"
        self.token = token
        self.next_id = 0

    def request(self, method: str, params: dict[str, Any] | None = None) -> Any:
        self.next_id += 1
        payload: dict[str, Any] = {
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": method,
        }
        if params is not None:
            payload["params"] = params
        request = urllib.request.Request(
            self.endpoint,
            data=json.dumps(payload).encode(),
            headers={
                "Authorization": f"Bearer {self.token}",
                "Content-Type": "application/json",
                "Accept": "application/json, text/event-stream",
                "MCP-Protocol-Version": "2025-06-18",
                "User-Agent": USER_AGENT,
            },
        )
        with urllib.request.urlopen(request, timeout=60) as response:
            content_type = response.headers.get("Content-Type", "")
            body = response.read().decode()
        if "text/event-stream" in content_type:
            data = [
                line[5:].strip()
                for line in body.splitlines()
                if line.startswith("data:")
            ]
            if not data:
                raise RuntimeError("MCP SSE response contained no data event")
            body = data[-1]
        message = json.loads(body)
        if "error" in message:
            raise RuntimeError(f"MCP {method} failed: {message['error']}")
        return message.get("result")

    def initialize(self) -> None:
        result = self.request(
            "initialize",
            {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "pricebook-smoke", "version": "0.1.0"},
            },
        )
        if not result or "serverInfo" not in result:
            raise AssertionError(f"unexpected initialize result: {result}")

    def call(self, name: str, arguments: dict[str, Any]) -> Any:
        result = self.request(
            "tools/call", {"name": name, "arguments": arguments}
        )
        if not result:
            raise AssertionError(f"{name} returned no result")
        if result.get("isError"):
            raise RuntimeError(f"{name} failed: {result.get('content')}")
        if "structuredContent" in result:
            return result["structuredContent"]
        content = result.get("content") or []
        if content and content[0].get("type") == "text":
            return json.loads(content[0]["text"])
        raise AssertionError(f"{name} returned unsupported content: {result}")


def search(client: McpClient, text: str) -> list[dict[str, Any]]:
    result = client.call(
        "search_products",
        {
            "queries": [
                {
                    "text": text,
                    "series": "S",
                    "domain": "kitchen",
                    "limit": 10,
                }
            ]
        },
    )
    return result["results"][0]["matches"]


def create_one(client: McpClient, tool: str, item: dict[str, Any]) -> int:
    result = client.call(tool, {"items": [item]})
    return result["created"][0]


def deletion_groups(record_groups: list[tuple[str, int]]) -> list[dict[str, Any]]:
    # Delete the variant last; its foreign keys cascade to several other tables.
    order = {
        "codes": 0,
        "prices": 1,
        "relations": 2,
        "pricing_rules": 3,
        "documents": 4,
        "variants": 5,
    }
    return [
        {"table": table, "ids": [record_id]}
        for table, record_id in sorted(
            record_groups, key=lambda record: order[record[0]]
        )
    ]


def run(base_url: str, token: str, writes: bool) -> None:
    health_request = urllib.request.Request(
        base_url.rstrip("/") + "/health", headers={"User-Agent": USER_AGENT}
    )
    with urllib.request.urlopen(health_request, timeout=30) as response:
        health = json.load(response)
    assert health["database"] == "d1", health
    print("health: ok")

    unauthorized = urllib.request.Request(
        base_url.rstrip("/") + "/mcp",
        data=b'{"jsonrpc":"2.0","id":0,"method":"tools/list"}',
        headers={"Content-Type": "application/json", "User-Agent": USER_AGENT},
    )
    try:
        urllib.request.urlopen(unauthorized, timeout=30)
    except urllib.error.HTTPError as error:
        assert error.code == 401, error.code
    else:
        raise AssertionError("MCP request without a token was not rejected")
    print("authentication rejection: ok")

    client = McpClient(base_url, token)
    client.initialize()
    print("initialize: ok")

    tools = client.request("tools/list")
    tool_names = {tool["name"] for tool in tools["tools"]}
    assert {"list_price_books", "search_products", "save_products"} <= tool_names
    print(f"tools/list: {len(tool_names)} tools")

    books = client.call("list_price_books", {})["price_books"]
    assert len(books) == 5, f"expected five active price books, got {len(books)}"
    print("list_price_books: 5 active books")

    matches = search(client, "铰链")
    assert matches, "catalogue search returned no hinges"
    print(f"search_products: {len(matches)} sample matches")

    colors = client.call("search_colors", {"queries": [{"text": "白", "limit": 3}]})
    assert colors["results"][0]["matches"], "color search returned no matches"
    print("search_colors: ok")

    product_id = matches[0]["product_id"]
    quote = client.call(
        "get_quote_data",
        {
            "series": "S",
            "domain": "kitchen",
            "selections": [{"product_id": product_id}],
            "max_variants_per_product": 5,
            "include_pricing_context": True,
        },
    )
    assert quote["items"][0]["product"]["product_id"] == product_id
    print(
        "get_quote_data: "
        f"{len(quote['items'][0]['product']['variants'])} variants, "
        f"{len(quote['pricing_rules'])} rules"
    )

    if not writes:
        return

    suffix = uuid.uuid4().hex[:12]
    name = f"D1 smoke {suffix}"
    rollback_name = f"D1 smoke rollback {suffix}"
    created_id: int | None = None
    record_groups: list[tuple[str, int]] = []

    try:
        try:
            client.call(
                "save_products",
                {
                    "items": [
                        {
                            "category_code": "hinge",
                            "name": rollback_name,
                            "scopes": [{"series": "S", "domain": "kitchen"}],
                        },
                        {
                            "category_code": "does-not-exist",
                            "name": rollback_name + " invalid",
                            "scopes": [{"series": "S", "domain": "kitchen"}],
                        },
                    ]
                },
            )
        except RuntimeError:
            pass
        else:
            raise AssertionError("invalid two-item write batch unexpectedly succeeded")
        assert not search(client, rollback_name), "failed batch partially committed"
        print("failed write batch: no partial commit")

        created = client.call(
            "save_products",
            {
                "items": [
                    {
                        "category_code": "hinge",
                        "name": name,
                        "properties": {"smoke_test": True},
                        "status": "active",
                        "scopes": [
                            {"series": "S", "domain": "kitchen"},
                            {"series": "P", "domain": "home"},
                        ],
                    }
                ]
            },
        )
        created_id = created["created"][0]
        found = search(client, name)
        assert any(item["product_id"] == created_id for item in found), found
        print(f"create product + generated ID/scopes: ok ({created_id})")

        book_id = next(
            book["id"]
            for book in books
            if book["series"] == "S" and book["domain"] == "kitchen"
        )
        variant_id = create_one(
            client,
            "save_variants",
            {
                "product_id": created_id,
                "variant_key": "default",
                "label": "D1 smoke variant",
                "properties": {"width_mm": 600},
                "constraints": {},
                "status": "active",
            },
        )
        record_groups.append(("variants", variant_id))
        code_id = create_one(
            client,
            "save_product_codes",
            {
                "product_id": created_id,
                "variant_id": variant_id,
                "code": f"D1SMOKE-{suffix}",
                "code_type": "model",
                "is_primary": True,
                "conditions": {},
            },
        )
        record_groups.append(("codes", code_id))
        price_id = create_one(
            client,
            "save_prices",
            {
                "price_book_id": book_id,
                "variant_id": variant_id,
                "price_kind": "factory",
                "amount_decimal": "1.23",
                "currency": "CNY",
                "unit": "piece",
                "measurement_basis": "fixed",
                "conditions": {},
            },
        )
        record_groups.append(("prices", price_id))
        relation_id = create_one(
            client,
            "save_relations",
            {
                "price_book_id": book_id,
                "parent_product_id": created_id,
                "parent_variant_id": variant_id,
                "child_label": "D1 smoke child",
                "relation_type": "optional",
                "quantity": {"value": 1},
                "conditions": {},
            },
        )
        record_groups.append(("relations", relation_id))
        rule_id = create_one(
            client,
            "save_pricing_rules",
            {
                "price_book_id": book_id,
                "product_id": created_id,
                "variant_id": variant_id,
                "category_code": "hinge",
                "code": f"D1SMOKE-{suffix}",
                "name": "D1 smoke rule",
                "rule_type": "note",
                "scope": {},
                "conditions": {},
                "data": {},
            },
        )
        record_groups.append(("pricing_rules", rule_id))
        document_id = create_one(
            client,
            "save_documents",
            {
                "price_book_id": book_id,
                "category_code": "hinge",
                "doc_type": "other",
                "title": "D1 smoke document",
                "content": suffix,
                "data": {"smoke_test": True},
            },
        )
        record_groups.append(("documents", document_id))
        print("variant/code/price/relation/rule/document creation: ok")

        deleted_records = client.call(
            "delete_records",
            {"groups": deletion_groups(record_groups)},
        )
        expected_record_ids = {record_id for _, record_id in record_groups}
        assert expected_record_ids <= set(deleted_records["deleted"]), deleted_records
        record_groups.clear()
        print("delete_records: ok")

        deleted = client.call(
            "delete_products",
            {"items": [{"product_id": created_id, "mode": "hard"}]},
        )
        assert created_id in deleted["deleted"], deleted
        assert not search(client, name), "hard-deleted product is still searchable"
        created_id = None
        print("hard delete + cleanup: ok")
    finally:
        if created_id is not None:
            if record_groups:
                client.call(
                    "delete_records",
                    {"groups": deletion_groups(record_groups)},
                )
            client.call(
                "delete_products",
                {"items": [{"product_id": created_id, "mode": "hard"}]},
            )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default="http://127.0.0.1:8787")
    parser.add_argument(
        "--token", default=os.environ.get("MCP_BEARER_TOKEN", "local-test-token")
    )
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    run(args.base_url, args.token, args.write)


if __name__ == "__main__":
    main()
