# Demo

A sample workspace showing Spring Boot multi-module + Node web.

## Install

```bash
git clone https://github.com/demo/demo.git
npm install
```

## Quick Start

```sh
mvn -pl user-api spring-boot:run
cd web && npm run dev
curl http://localhost:8081/actuator/health
mvn clean package
```

参数说明见 `mvn -pl user-api spring-boot:run -DskipTests`。
