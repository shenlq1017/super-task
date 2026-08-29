package com.example.order;

import java.util.LinkedHashMap;
import java.util.Map;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping("/api/order")
public class OrderController {

    @Value("${server.port:8080}")
    private String port;

    @GetMapping("/ping")
    public Map<String, Object> ping() {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("service", "order-api");
        out.put("port", port);
        out.put("ok", true);
        return out;
    }
}
