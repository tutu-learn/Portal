#!/usr/bin/env bash
set -euo pipefail

BASE="http://localhost:8000"
COOKIE_JAR=$(mktemp)
SUFFIX=$(date +%s%N | tail -c 6)

curl_json() {
    curl -s -c "$COOKIE_JAR" -b "$COOKIE_JAR" "$@"
}

echo "=== LOGIN ==="
curl_json "$BASE/api/method/login" -X POST -d "usr=Administrator&pwd=admin" | python3 -m json.tool

echo ""
echo "=== WORKFLOW 1: system_user_block_modules ==="
EMAIL1="sys_user_${SUFFIX}@example.com"
CREATE1=$(curl_json "$BASE/api/method/frappe.client.insert" \
    -H "Content-Type: application/json" \
    -d "{\"doc\":{\"doctype\":\"User\",\"email\":\"$EMAIL1\",\"first_name\":\"Sys User $SUFFIX\",\"user_type\":\"System User\",\"roles\":[{\"role\":\"System Manager\"}]}}")
echo "CREATE sys user: $CREATE1"

SAVE1=$(curl_json "$BASE/api/method/frappe.client.save" \
    -H "Content-Type: application/json" \
    -d "{\"doc\":{\"doctype\":\"User\",\"name\":\"$EMAIL1\",\"first_name\":\"Sys User $SUFFIX\",\"enabled\":1,\"user_type\":\"System User\",\"roles\":[{\"role\":\"System Manager\"}],\"block_modules\":[{\"module\":\"Contacts\"}]}}")
echo "SAVE sys user block_modules: $SAVE1"

GET1=$(curl_json "$BASE/api/method/frappe.client.get" \
    -H "Content-Type: application/json" \
    -d "{\"doctype\":\"User\",\"name\":\"$EMAIL1\"}")
echo "GET sys user: $GET1"

GETDOC1=$(curl_json "$BASE/api/method/frappe.desk.form.load.getdoc?doctype=User&name=$EMAIL1")
echo "GETDOC sys user: $GETDOC1"

echo ""
echo "=== WORKFLOW 2: website_user_block_modules_api ==="
EMAIL2="web_user_${SUFFIX}@example.com"
CREATE2=$(curl_json "$BASE/api/method/frappe.client.insert" \
    -H "Content-Type: application/json" \
    -d "{\"doc\":{\"doctype\":\"User\",\"email\":\"$EMAIL2\",\"first_name\":\"Web User $SUFFIX\",\"user_type\":\"Website User\"}}")
echo "CREATE web user: $CREATE2"

SAVE2=$(curl_json "$BASE/api/method/frappe.client.save" \
    -H "Content-Type: application/json" \
    -d "{\"doc\":{\"doctype\":\"User\",\"name\":\"$EMAIL2\",\"first_name\":\"Web User $SUFFIX\",\"enabled\":1,\"user_type\":\"Website User\",\"block_modules\":[{\"module\":\"Automation\"}]}}")
echo "SAVE web user block_modules: $SAVE2"

GET2=$(curl_json "$BASE/api/method/frappe.client.get" \
    -H "Content-Type: application/json" \
    -d "{\"doctype\":\"User\",\"name\":\"$EMAIL2\"}")
echo "GET web user: $GET2"

echo ""
echo "=== WORKFLOW 3: module_profile_blocks_modules ==="
PROF_NAME="Profile $SUFFIX"
CREATE_PROF=$(curl_json "$BASE/api/method/frappe.client.insert" \
    -H "Content-Type: application/json" \
    -d "{\"doc\":{\"doctype\":\"Module Profile\",\"module_profile_name\":\"$PROF_NAME\",\"block_modules\":[{\"module\":\"Custom\"}]}}")
echo "CREATE module profile: $CREATE_PROF"

EMAIL3="profile_user_${SUFFIX}@example.com"
CREATE3=$(curl_json "$BASE/api/method/frappe.client.insert" \
    -H "Content-Type: application/json" \
    -d "{\"doc\":{\"doctype\":\"User\",\"email\":\"$EMAIL3\",\"first_name\":\"Profile User $SUFFIX\",\"user_type\":\"System User\",\"module_profile\":\"$PROF_NAME\",\"roles\":[{\"role\":\"System Manager\"}]}}")
echo "CREATE profile user: $CREATE3"

GET3=$(curl_json "$BASE/api/method/frappe.client.get" \
    -H "Content-Type: application/json" \
    -d "{\"doctype\":\"User\",\"name\":\"$EMAIL3\"}")
echo "GET profile user: $GET3"

echo ""
echo "=== CLEANUP ==="
for DOCNME in "$EMAIL1" "$EMAIL2" "$EMAIL3"; do
    DEL=$(curl_json -X DELETE "$BASE/api/resource/User/$DOCNME")
    echo "DELETE User $DOCNME: $DEL"
done
DEL_PROF=$(curl_json -X DELETE "$BASE/api/resource/Module%20Profile/$(python3 -c "import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1]))" "$PROF_NAME")")
echo "DELETE Module Profile '$PROF_NAME': $DEL_PROF"

rm -f "$COOKIE_JAR"
